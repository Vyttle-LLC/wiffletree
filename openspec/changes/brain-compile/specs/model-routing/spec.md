## MODIFIED Requirements

<!-- Delta order for "Who runs what": 1 assistant-spaces-jobs-boards (Assistant, Router, Job) → 2 brain-compile (this), which restates that version and adds the Brain compiler row. The table ends with Assistant, Router, Job and Brain compiler; there is no Issue assistant row. -->

### Requirement: Who runs what
The host SHALL persist, per machine, one row per job: Coordinator, Implementer, Tester, one per configured verifier keyed by its focus, optionally PR keeper, Assistant, Router, optionally Job and optionally Brain compiler. Each row SHALL have a first choice (provider, model, effort), a then choice that is a model, wait (the default, saved as no then) or skip, and an optional small-work line with its own first and then. The Brain compiler row SHALL have no small-work line. Skip SHALL be offered only on verifier rows, on either line. A save SHALL be refused, whole, with the previous table kept, when a row of Coordinator, Implementer, Tester, Assistant or Router is missing or doubled, two rows name one verifier or two PR keeper, Job or Brain compiler rows exist, a model or effort is empty or too long, a then choice uses the same provider as its first choice, a Coordinator, Implementer, Tester, PR keeper, Assistant, Router, Job or Brain compiler line's then is skip, or the Brain compiler row has a small-work line. A table saved before skip existed, or before the Brain compiler row existed, SHALL load unchanged. A table saved before the Assistant and Router rows existed SHALL load with an Assistant row copied from the Implementer row's small-work line, or its main line without one, and a Router row on the smallest model the Assistant row's provider lists at low effort, both named in the table's `copied` list until a save stores them. Without a Job row, jobs SHALL resolve the Assistant row. The table SHALL NOT be shared through the settings repository. A save that also changes the guide or the switch threshold SHALL change all of them or none, and a save together with the review settings SHALL be checked whole before anything is written.

The table SHALL match the configured verifiers whenever it is read: a row for a verifier that is no longer configured SHALL be left out, and a configured verifier without a row SHALL use a copy of the first reviewer's row, or of the Tester row with no reviewer row, and SHALL be named in the table's `copied` list until a save stores its row.

Assistant threads, issue threads included, SHALL resolve the Assistant row, the router the Router row and job runs the Job row, when each thread, router run or job run starts, by the same rules as an agent start. A Space's default model for threads or jobs, when this Mac may use it, and a model the user chose in a thread's composer or a job's recipe SHALL win over the row. When every Router choice is spent or cannot be chosen, the router SHALL count as unsure instead of waiting (see assistant-router). Brain compile runs SHALL resolve the Brain compiler row, never the Job row (see "The Brain compiler's row").

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
- **WHEN** a machine whose Implementer small-work line is Sonnet · high then GPT 6.1 Sol · medium starts this release
- **THEN** the Assistant row is Sonnet · high then GPT 6.1 Sol · medium, the Router row is Haiku · low, the Models page notes both as copied, and there is no Job row

#### Scenario: Jobs follow the Assistant row
- **WHEN** the table has no Job row and the Assistant row is Sonnet · medium
- **THEN** a job run whose recipe and Space name no model starts on Sonnet · medium

#### Scenario: The router never waits
- **WHEN** Claude is spent and the Router row is Haiku · low then wait
- **THEN** the router counts as unsure and the user is asked where the message goes

#### Scenario: A table from before the Brain compiler row
- **WHEN** the host loads a table saved before the Brain compiler row existed
- **THEN** it loads unchanged, with no Brain compiler row

#### Scenario: The last rows
- **WHEN** the human opens the Models page with every optional row set
- **THEN** the table ends with Assistant, Router, Job and Brain compiler, and has no Issue assistant row

## ADDED Requirements

### Requirement: The Brain compiler's row
A brain's compile runs SHALL use the table's Brain compiler row, resolved like any agent start when a run needs a model. With no row, no compile run SHALL start a model, and the Brains page SHALL say to choose a Brain compiler model in Models. A run that finds nothing pending SHALL NOT resolve the row. Only the human's client SHALL change the row. The Models page SHALL show the Brain compiler row with a No model choice, as the PR keeper row does, and its then menu SHALL NOT offer skip.

#### Scenario: Nothing chosen
- **WHEN** the table has no Brain compiler row and the nightly run of `sagehaven-docs` finds pending entries
- **THEN** no model starts, the run is recorded as waiting for a Brain compiler model, and the Brains page says to choose one in Models

#### Scenario: A row while Claude is spent
- **WHEN** the Brain compiler row is Sonnet · medium then GPT 6.1 Sol · medium and Claude is spent
- **THEN** the run starts on GPT 6.1 Sol · medium
