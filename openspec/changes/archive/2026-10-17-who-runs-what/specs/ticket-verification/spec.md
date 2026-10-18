## REMOVED Requirements

### Requirement: One call starts any number of verifiers
**Reason**: `verify_ticket` no longer takes a model per verifier; see "One call starts every verifier of a level".
**Migration**: Coordinators drop `verifiers` from their calls.

## ADDED Requirements

### Requirement: One call starts every verifier of a level
`verify_ticket` SHALL take a `ticket_id`, the name of a review level as `level`, and an optional `small` that applies to the whole level. The level's name SHALL be matched ignoring surrounding spaces and ASCII case. There SHALL be no default level. It SHALL refuse with nothing started when `level` is missing or names no configured level, naming the configured levels; when a level names a verifier that is not configured; when it is sent `verifiers`, `profile` or `reason`; or when a verifier's row has no choice that can run on this machine. Each verifier of the level SHALL resolve its row (see model-routing). An existing verifier session SHALL be reused only when its pinned profile equals the resolved one; otherwise the host SHALL archive it and start a fresh verifier, never changing a pinned model. On success it SHALL record the ticket worktree's HEAD commit as the round's pinned commit, assign or resume one agent per verifier of the level and none for any other verifier, pin each new verifier with Wiffletree's reason, send each one message naming the pinned commit, and set the ticket's state to `verifying`. A verifier that must wait for a reset SHALL be pinned and held as model-routing says, and SHALL NOT stop the others. A verifier with the same role and focus SHALL be the same session in every round of a cycle and keep its model. A cycle's verifiers SHALL be fixed when it starts: a later round SHALL start only verifiers of that cycle whose result failed, and a change to the levels while the cycle runs SHALL NOT affect it. A later cycle MAY name a different level. `verify_ticket` SHALL also refuse when the ticket's state is not `ready_for_testing`, except a finished ticket that was never verified (no verification record, state `passed` or `failed`), when its worktree holds uncommitted or untracked files, or when a cycle is already running, naming the reason. It SHALL NOT refuse because of how many verifiers are configured.

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

## MODIFIED Requirements

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

### Requirement: The host may start a review for the PR keeper
The host SHALL be able to start one verification cycle on a ticket for the PR keeper, without a `verify_ticket` call and without the ticket being `ready_for_testing`, only for a commit the keeper has moved onto the ticket's branch. The cycle SHALL run the level the keeper's settings name, whose kind SHALL be re-check, and every rule for starting a level SHALL apply unchanged: the ticket's latest cycle passed, and the level leaves out no configured verifier that has an open finding. It SHALL NOT start while the ledger holds any `open` entry, so that the review can only add entries and never change one. It SHALL be one round. Each verifier SHALL resolve its row as in any cycle. When any of this cannot be met, no cycle SHALL start and nothing about the ticket SHALL change. The cycle SHALL be recorded as the keeper's, SHALL take the next cycle number, and SHALL NOT count against the cycle cap. When it starts the cycle, the host SHALL record on the ticket, for every keeper review whether or not a model made the commit, the cycle's number, the commit the branch had before the keeper moved it and the ticket's state at that moment; a withdrawal uses that record and nothing else. The Tickets panel and `workspace_context` SHALL mark it as the PR keeper's. Verifiers SHALL get the same message and SHALL be bound by the same read-only and pinned-commit rules as in any cycle.

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
