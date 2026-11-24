## MODIFIED Requirements

### Requirement: Who runs what
The host SHALL persist, per machine, one row per job: Coordinator, Implementer, Tester, one per configured verifier keyed by its focus, optionally PR keeper, Assistant, optionally Job and optionally Brain compiler. Each row SHALL have a first choice (provider, model, effort), a then choice that is a model, wait (the default, saved as no then) or skip, and an optional small-work line with its own first and then. The Brain compiler row SHALL have no small-work line. Skip SHALL be offered only on verifier rows, on either line. A save SHALL be refused, whole, with the previous table kept, when a row of Coordinator, Implementer, Tester or Assistant is missing or doubled, two rows name one verifier or two PR keeper, Job or Brain compiler rows exist, a model or effort is empty or too long, a then choice uses the same provider as its first choice, a Coordinator, Implementer, Tester, PR keeper, Assistant, Job or Brain compiler line's then is skip, or the Brain compiler row has a small-work line. A table saved before skip existed, or before the Brain compiler row existed, SHALL load unchanged. A table saved before the Assistant row existed SHALL load with an Assistant row copied from the Implementer row's small-work line, or its main line without one, named in the table's `copied` list until a save stores it. A table saved with a Router row SHALL load without it, and the next save SHALL drop it. Without a Job row, jobs SHALL resolve the Assistant row. The table SHALL NOT be shared through the settings repository. A save that also changes the guide or the switch threshold SHALL change all of them or none, and a save together with the review settings SHALL be checked whole before anything is written.

The table SHALL match the configured verifiers whenever it is read: a row for a verifier that is no longer configured SHALL be left out, and a configured verifier without a row SHALL use a copy of the first reviewer's row, or of the Tester row with no reviewer row, and SHALL be named in the table's `copied` list until a save stores its row.

The Assistant's conversation, issue threads and job runs SHALL resolve the Assistant row, or for job runs the Job row, when the conversation's provider session, each issue thread or each job run starts, by the same rules as an agent start. A model the user chose in the composer or a job's recipe SHALL win over the row. A row of an issue thread's or a job run's Space SHALL override this Mac's (see settings-cascade); the conversation, which belongs to no Space, SHALL resolve this Mac's row. The coordinators of Assistant work, and their workers, SHALL resolve their own rows as every project's agents do. Brain compile runs SHALL resolve the Brain compiler row, never the Job row (see "The Brain compiler's row").

#### Scenario: A then on the same subscription
- **WHEN** the human saves Implementer with Opus · medium then Sonnet · high
- **THEN** the save is refused, saying both choices use the Claude subscription, and the previous table stays

#### Scenario: A verifier from another machine
- **WHEN** a pulled review setting adds the reviewer Security, which has no row on this machine
- **THEN** Security resolves with a copy of the first reviewer's row, the Models page notes it, and saving the page stores the row

#### Scenario: Skip on a row that is not a verifier
- **WHEN** the human saves the Implementer row as Opus · medium then skip
- **THEN** the save is refused, saying only reviews can be skipped, and the previous table stays

#### Scenario: Rows added by the upgrade
- **WHEN** a machine whose Implementer small-work line is Sonnet · high then GPT 6.1 Sol · medium starts a release with an Assistant row for the first time
- **THEN** the Assistant row is Sonnet · high then GPT 6.1 Sol · medium, the Models page notes it as copied, and there is no Job row

#### Scenario: The router never waits
- **WHEN** Claude is spent and the host loads a table saved with a Router row on Haiku · low then wait
- **THEN** the table loads without a Router row, no router run starts or waits, the human's message goes straight to the conversation, and the next save writes no Router row

#### Scenario: Jobs follow the Assistant row
- **WHEN** the table has no Job row and the Assistant row is Sonnet · medium
- **THEN** a job run whose recipe and Space name no model starts on Sonnet · medium

#### Scenario: A table from before the Brain compiler row
- **WHEN** the host loads a table saved before the Brain compiler row existed
- **THEN** it loads unchanged, with no Brain compiler row

#### Scenario: The last rows
- **WHEN** the human opens the Models page with every optional row set
- **THEN** the table ends with Assistant, Job and Brain compiler, and has no Router or Issue assistant row
