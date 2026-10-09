## MODIFIED Requirements

### Requirement: Configured verifiers
The workspace SHALL hold one verification setting: an ordered list of verifiers and a round cap. Each verifier SHALL name a role (`tester` or `reviewer`), a short focus that is unique in the list, an optional instruction, and optionally a provider that limits the coordinator's model choice for that verifier. A saved `size` SHALL be ignored. The round cap SHALL default to 2 and accept values from 1 to 5. With no saved setting, the list SHALL be a tester (focus "Tests"), a Claude reviewer (focus "Claude") and a Codex reviewer (focus "Codex"), which fits the default project turn limit of 4. The setting SHALL be edited only on the Models page's Review card.

#### Scenario: Default setting
- **WHEN** the human has never saved a verification setting
- **THEN** `verify_ticket` starts a tester, a Claude reviewer and a Codex reviewer, and the round cap is 2

#### Scenario: Invalid setting
- **WHEN** the human saves two verifiers with the same focus, or a round cap of 0
- **THEN** the setting is rejected and the previous setting stays in effect

### Requirement: One call starts every verifier
`verify_ticket` SHALL take a `ticket_id` and a `verifiers` list with one `{focus, profile, reason}` entry per configured verifier. It SHALL refuse with nothing started when an entry is missing, duplicated or unknown; when a reason is not one line; when a profile's provider differs from the verifier's configured provider or, without one, is outside its role's providers (a reviewer may use any enabled provider); when a profile is not allowed on this machine; or when an existing verifier session it would reuse would be held. An existing verifier session SHALL be reused only when its pinned profile equals the submitted one; otherwise the host SHALL archive it and start a fresh verifier on the submitted profile and reason, never changing a pinned model. Provider and model refusals SHALL record a `model_rejected` event, and the host SHALL never substitute a model. On success it SHALL record the ticket worktree's HEAD commit as the round's pinned commit, assign or resume one agent per configured verifier, pin each new verifier to its choice with the coordinator and reason recorded like an assignment, send each one message naming the pinned commit, and set the ticket's state to `verifying`. A verifier with the same role and focus SHALL be the same session in every round of a cycle and keep its model. `verify_ticket` SHALL also refuse when the ticket's state is not `ready_for_testing`, except a finished ticket that was never verified (no verification record, state `passed` or `failed`), when its worktree holds uncommitted or untracked files, when a cycle is already running, or when the number of verifiers exceeds the smaller of the project's turn limit less one and the host-wide worker turn limit, naming the reason.

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
