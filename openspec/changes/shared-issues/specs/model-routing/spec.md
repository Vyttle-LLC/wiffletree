## MODIFIED Requirements

### Requirement: Who runs what
The host SHALL persist, per machine, one row per job: Coordinator, Implementer, Tester, one per configured verifier keyed by its focus, optionally PR keeper, and optionally Issue assistant. Each row SHALL have a first choice (provider, model, effort), a then choice that is a model, wait (the default, saved as no then) or skip, and an optional small-work line with its own first and then. Skip SHALL be offered only on verifier rows, on either line. A save SHALL be refused, whole, with the previous table kept, when a row of Coordinator, Implementer or Tester is missing or doubled, two rows name one verifier, two PR keeper rows or two Issue assistant rows exist, a model or effort is empty or too long, a then choice uses the same provider as its first choice, or a Coordinator, Implementer, Tester, PR keeper or Issue assistant line's then is skip. A table saved before skip existed SHALL load unchanged. The table SHALL NOT be shared through the settings repository. A save that also changes the guide or the switch threshold SHALL change all of them or none, and a save together with the review settings SHALL be checked whole before anything is written.

The table SHALL match the configured verifiers whenever it is read: a row for a verifier that is no longer configured SHALL be left out, and a configured verifier without a row SHALL use a copy of the first reviewer's row, or of the Tester row with no reviewer row, and SHALL be named in the table's `copied` list until a save stores its row.

#### Scenario: A then on the same subscription
- **WHEN** the human saves Implementer with Opus · medium then Sonnet · high
- **THEN** the save is refused, saying both choices use the Claude subscription, and the previous table stays

#### Scenario: A verifier from another machine
- **WHEN** a pulled review setting adds the reviewer Security, which has no row on this machine
- **THEN** Security resolves with a copy of the first reviewer's row, the Models page notes it, and saving the page stores the row

#### Scenario: Skip on a row that is not a verifier
- **WHEN** the human saves the Implementer row as Opus · medium then skip
- **THEN** the save is refused, saying only reviews can be skipped, and the previous table stays

#### Scenario: An Issue assistant row
- **WHEN** the human saves an Issue assistant row of Sonnet · low then wait
- **THEN** the table holds the row, and the next assistant turn resolves Sonnet · low

#### Scenario: A table from before issues
- **WHEN** a table saved before this change is read
- **THEN** it loads unchanged with no Issue assistant row
