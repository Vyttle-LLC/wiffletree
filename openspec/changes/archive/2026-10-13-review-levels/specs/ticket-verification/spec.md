## MODIFIED Requirements

### Requirement: Configured verifiers
The workspace SHALL hold one verification setting: an ordered list of verifiers, a list of review levels (see Review levels), a round cap and a cycle cap. Each verifier SHALL name a role (`tester` or `reviewer`), a short focus that is unique in the list, an optional instruction, and optionally a provider that limits the coordinator's model choice for that verifier. A saved `size` SHALL be ignored. The round cap SHALL default to 3, the cycle cap SHALL default to 2, and each SHALL accept values from 1 to 5. With no saved setting, the list SHALL be a tester (focus "Tests"), a Claude reviewer (focus "Correctness", instruction "Correctness against the ticket's acceptance criteria.") and a Codex reviewer (focus "Regressions", instruction "Regressions and test coverage."), and the levels SHALL be one level named "Full", a full review that runs all three. A saved setting SHALL keep its verifiers and caps; a saved setting without a cycle cap SHALL use the default. The setting SHALL be edited only on the Models page's Review card.

#### Scenario: Default setting
- **WHEN** the human has never saved a verification setting
- **THEN** `verify_ticket` naming the level "Full" starts a tester "Tests", a Claude reviewer "Correctness" and a Codex reviewer "Regressions", each reviewer's instruction naming its lens, with a round cap of 3 and a cycle cap of 2

#### Scenario: Saved setting is not migrated
- **WHEN** the human saved a list with reviewers "Claude" and "Codex" and a round cap of 2 before review levels existed
- **THEN** `verify_ticket` naming the level "Full" keeps starting those reviewers with a round cap of 2, and the cycle cap is 2

#### Scenario: Invalid setting
- **WHEN** the human saves two verifiers with the same focus, a round cap of 0 or a cycle cap of 6
- **THEN** the setting is rejected and the previous setting stays in effect

### Requirement: Cycle outcomes reach the coordinator once
Verifier verdicts and the implementer's `ready_for_testing` reports during a running cycle SHALL reach the project coordinator in the next batch, like progress, and SHALL NOT wake it. Check-ins are the one exception to this rule against a coordinator turn during a round: an agent's check-in, for a 30-minute turn or for its turn count, SHALL wake the project coordinator even while its ticket's cycle is running, and SHALL NOT count as a verdict or change the ticket's state or cycle. Other implementer reports follow the ticket-state rule above and wake it. The host SHALL wake the project coordinator once when the cycle ends: with state `passed` when the latest result of every verifier of the cycle's level passed, or with state `blocked` when a round at the cap still has a failed result, a failed round has no unarchived implementer to send its findings to, or any verifier reported `blocked`. The message SHALL be sent from Wiffletree and SHALL name the cycle number, the cycle's level and whether it was a re-check, for a full review which of the full reviews the cycle cap allows it was, the rounds used and the round cap, every `open` ledger entry, every `untriaged` entry, and the ids of entries already decided, and SHALL include the report of any verifier that reported `blocked`. It SHALL offer only the actions allowed at that point: after a pass, triaging the untriaged entries and accepting; after a block, sending fixes and verifying again with a full review only while the cycle cap allows another one, and never with a re-check, accepting with a waiver only when no verifier reported `blocked`, closing the ticket, or asking the human. It SHALL NOT suggest starting a fresh cycle after a pass. Reaching a cap SHALL NOT create a question for the human; the project coordinator decides whether to ask. A later `verify_ticket` call SHALL start a new cycle with every verifier of the level it names.

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

### Requirement: Cycle cap
`verify_ticket` SHALL refuse a level whose kind is full review, starting nothing, when the ticket has already started as many full reviews as the cycle cap allows, saying to accept with a waiver, close the ticket or ask the human. Every started full review SHALL count, including one an implementer's report ended early and a cycle recorded without a level. A re-check SHALL NOT count against the cycle cap and SHALL NOT be refused by it. Cycle numbers SHALL stay one sequence across levels.

#### Scenario: Third cycle refused
- **WHEN** a ticket has run two full reviews under a cycle cap of 2 and the coordinator calls `verify_ticket` naming a full review
- **THEN** the call is refused with that guidance and no verifier is started

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

### Requirement: One call starts any number of verifiers
`verify_ticket` SHALL take a `ticket_id`, the name of a review level as `level`, and a `verifiers` list with one `{focus, profile, reason}` entry per verifier of that level. The level's name SHALL be matched ignoring surrounding spaces and ASCII case. There SHALL be no default level. It SHALL refuse with nothing started when `level` is missing or names no configured level, naming the configured levels; when a level names a verifier that is not configured; when an entry is missing, duplicated, unknown or for a verifier outside the level; when a reason is not one line; when a profile's provider differs from the verifier's configured provider, or is outside its role's providers (a reviewer may use any enabled provider), so a tester or reviewer with a configured provider must satisfy both; when a profile is not allowed on this machine; or when an existing verifier session it would reuse would be held. An existing verifier session SHALL be reused only when its pinned profile equals the submitted one; otherwise the host SHALL archive it and start a fresh verifier on the submitted profile and reason, never changing a pinned model. Provider and model refusals SHALL record a `model_rejected` event, and the host SHALL never substitute a model. On success it SHALL record the ticket worktree's HEAD commit as the round's pinned commit, assign or resume one agent per verifier of the level and none for any other verifier, pin each new verifier to its choice with the coordinator and reason recorded like an assignment, send each one message naming the pinned commit, and set the ticket's state to `verifying`. A verifier with the same role and focus SHALL be the same session in every round of a cycle and keep its model. A cycle's verifiers SHALL be fixed when it starts: a later round SHALL start only verifiers of that cycle whose result failed, and a change to the levels while the cycle runs SHALL NOT affect it. A later cycle MAY name a different level. `verify_ticket` SHALL also refuse when the ticket's state is not `ready_for_testing`, except a finished ticket that was never verified (no verification record, state `passed` or `failed`), when its worktree holds uncommitted or untracked files, or when a cycle is already running, naming the reason. It SHALL NOT refuse because of how many verifiers are configured.

#### Scenario: Three verifiers from one call
- **WHEN** the level "Full" runs three verifiers and the project coordinator calls `verify_ticket` once, naming "Full", with an allowed profile and reason for each on a ticket that is `ready_for_testing`
- **THEN** three verifier sessions each receive a message naming the same commit, each pinned to its chosen profile with its reason
- **AND** the ticket's state is `verifying`

#### Scenario: Unready ticket
- **WHEN** `verify_ticket` is called on a ticket whose implementer has not reported `ready_for_testing`
- **THEN** the host refuses and starts no verifier

#### Scenario: Wrong provider for a verifier
- **WHEN** a Codex tester verifier is given a Claude profile
- **THEN** the call fails with "Claude is not configured for Tester · Codex. Tester · Codex uses: Codex.", no verifier session is created and a `model_rejected` event is recorded

#### Scenario: Seven verifiers
- **WHEN** a level runs seven verifiers and the coordinator calls `verify_ticket` naming it with a choice for each
- **THEN** seven verifier sessions each receive a message naming the same commit, and the ticket's state is `verifying`

#### Scenario: Verifier provider changed between cycles
- **WHEN** a verifier ran cycle 1 on a Claude model, its provider is changed to Codex, and the next `verify_ticket` gives it a Codex profile
- **THEN** cycle 2 runs on a fresh Codex verifier session with that profile and reason, and the Claude session is archived with its model unchanged

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
- **THEN** `verify_ticket` naming "Light" with a choice for Correctness starts one verifier session, and Tests, Regressions and Style receive no message

#### Scenario: Choice for a verifier outside the level
- **WHEN** `verify_ticket` names "Light", which runs only Correctness, with choices for Correctness and Tests
- **THEN** the call is refused, saying Tests is not part of Light, and no verifier is started

#### Scenario: Choice missing for a verifier of the level
- **WHEN** `verify_ticket` names "Full", which runs Tests, Correctness and Regressions, with choices for Tests and Correctness only
- **THEN** the call is refused, asking for a profile and reason for the Regressions reviewer, and no verifier is started

#### Scenario: A level names a verifier that is not configured
- **WHEN** a hand-edited settings file has the level "Light" naming the focus "Style" and no verifier has that focus
- **THEN** `verify_ticket` naming "Light" is refused, saying why, and no verifier is started

#### Scenario: A later cycle at another level
- **WHEN** cycle 1 ran at "Full" and passed, the implementer commits a rebase and reports `ready_for_testing`, and `verify_ticket` names "Light"
- **THEN** cycle 2 starts with only Light's verifiers, and cycle 1's record still names "Full"

#### Scenario: Levels edited while a cycle runs
- **WHEN** round 1 of a cycle at "Full" failed for the Correctness reviewer, the human then unticks Correctness in "Full", and the implementer reports `ready_for_testing`
- **THEN** round 2 starts with the Correctness reviewer

## ADDED Requirements

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
