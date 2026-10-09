## MODIFIED Requirements

### Requirement: Configured verifiers
The workspace SHALL hold one verification setting: an ordered list of verifiers, a round cap and a cycle cap. Each verifier SHALL name a role (`tester` or `reviewer`), a short focus that is unique in the list, an optional instruction, and optionally a provider that limits the coordinator's model choice for that verifier. A saved `size` SHALL be ignored. The round cap SHALL default to 3, the cycle cap SHALL default to 2, and each SHALL accept values from 1 to 5. With no saved setting, the list SHALL be a tester (focus "Tests"), a Claude reviewer (focus "Correctness", instruction "Correctness against the ticket's acceptance criteria.") and a Codex reviewer (focus "Regressions", instruction "Regressions and test coverage."). A saved setting SHALL keep its verifiers and caps; a saved setting without a cycle cap SHALL use the default. The setting SHALL be edited only on the Models page's Review card.

#### Scenario: Default setting
- **WHEN** the human has never saved a verification setting
- **THEN** `verify_ticket` starts a tester "Tests", a Claude reviewer "Correctness" and a Codex reviewer "Regressions", each reviewer's instruction naming its lens, with a round cap of 3 and a cycle cap of 2

#### Scenario: Saved setting is not migrated
- **WHEN** the human saved a list with reviewers "Claude" and "Codex" and a round cap of 2 before this change
- **THEN** `verify_ticket` keeps starting those reviewers with a round cap of 2, and the cycle cap is 2

#### Scenario: Invalid setting
- **WHEN** the human saves two verifiers with the same focus, a round cap of 0 or a cycle cap of 6
- **THEN** the setting is rejected and the previous setting stays in effect

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

### Requirement: Cycle outcomes reach the coordinator once
Verifier verdicts and the implementer's `ready_for_testing` reports during a running cycle SHALL reach the project coordinator in the next batch, like progress, and SHALL NOT wake it. Check-ins are the one exception to this rule against a coordinator turn during a round: an agent's check-in, for a 30-minute turn or for its turn count, SHALL wake the project coordinator even while its ticket's cycle is running, and SHALL NOT count as a verdict or change the ticket's state or cycle. Other implementer reports follow the ticket-state rule above and wake it. The host SHALL wake the project coordinator once when the cycle ends: with state `passed` when every configured verifier's latest result passed, or with state `blocked` when a round at the cap still has a failed result, a failed round has no unarchived implementer to send its findings to, or any verifier reported `blocked`. The message SHALL be sent from Wiffletree and SHALL name the cycle number and the cycle cap, the rounds used and the round cap, every `open` ledger entry, every `untriaged` entry, and the ids of entries already decided, and SHALL include the report of any verifier that reported `blocked`. It SHALL offer only the actions allowed at that point: after a pass, triaging the untriaged entries and accepting; after a block, sending fixes and verifying again only while the cycle cap allows another cycle, accepting with a waiver only when no verifier reported `blocked`, closing the ticket, or asking the human. It SHALL NOT suggest starting a fresh cycle after a pass. Reaching a cap SHALL NOT create a question for the human; the project coordinator decides whether to ask. A later `verify_ticket` call within the cycle cap SHALL start a new cycle with every configured verifier.

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

### Requirement: One call starts any number of verifiers
`verify_ticket` SHALL take a `ticket_id` and a `verifiers` list with one `{focus, profile, reason}` entry per configured verifier. It SHALL refuse with nothing started when an entry is missing, duplicated or unknown; when a reason is not one line; when a profile's provider differs from the verifier's configured provider, or is outside its role's providers (a reviewer may use any enabled provider), so a tester or reviewer with a configured provider must satisfy both; when a profile is not allowed on this machine; or when an existing verifier session it would reuse would be held. An existing verifier session SHALL be reused only when its pinned profile equals the submitted one; otherwise the host SHALL archive it and start a fresh verifier on the submitted profile and reason, never changing a pinned model. Provider and model refusals SHALL record a `model_rejected` event, and the host SHALL never substitute a model. On success it SHALL record the ticket worktree's HEAD commit as the round's pinned commit, assign or resume one agent per configured verifier, pin each new verifier to its choice with the coordinator and reason recorded like an assignment, send each one message naming the pinned commit, and set the ticket's state to `verifying`. A verifier with the same role and focus SHALL be the same session in every round of a cycle and keep its model. `verify_ticket` SHALL also refuse when the ticket's state is not `ready_for_testing`, except a finished ticket that was never verified (no verification record, state `passed` or `failed`), when its worktree holds uncommitted or untracked files, or when a cycle is already running, naming the reason. It SHALL NOT refuse because of how many verifiers are configured.

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

#### Scenario: Seven verifiers
- **WHEN** seven verifiers are configured and the coordinator calls `verify_ticket` with a choice for each
- **THEN** seven verifier sessions each receive a message naming the same commit, and the ticket's state is `verifying`

#### Scenario: Verifier provider changed between cycles
- **WHEN** a verifier ran cycle 1 on a Claude model, its provider is changed to Codex, and the next `verify_ticket` gives it a Codex profile
- **THEN** cycle 2 runs on a fresh Codex verifier session with that profile and reason, and the Claude session is archived with its model unchanged

## REMOVED Requirements

### Requirement: One call starts every verifier
**Reason**: Replaced by "One call starts any number of verifiers", which is the same contract without the turn-limit refusals and their two scenarios.
**Migration**: None. A `verify_ticket` call that was refused for its verifier count now starts every verifier.
