## MODIFIED Requirements

### Requirement: Who runs what
The host SHALL persist, per machine, one row per job: Coordinator, Implementer, Tester, one per configured verifier keyed by its focus, and optionally PR keeper. Each row SHALL have a first choice (provider, model, effort), a then choice that is a model, wait (the default, saved as no then) or skip, and an optional small-work line with its own first and then. Skip SHALL be offered only on verifier rows, on either line. A save SHALL be refused, whole, with the previous table kept, when a row of Coordinator, Implementer or Tester is missing or doubled, two rows name one verifier or two PR keeper rows exist, a model or effort is empty or too long, a then choice uses the same provider as its first choice, or a Coordinator, Implementer, Tester or PR keeper line's then is skip. A table saved before skip existed SHALL load unchanged. The table SHALL NOT be shared through the settings repository. A save that also changes the guide or the switch threshold SHALL change all of them or none, and a save together with the review settings SHALL be checked whole before anything is written.

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

### Requirement: Resolution when an agent starts
Wiffletree SHALL resolve a job's row only when an agent starts: an `assign_ticket` that creates an agent, each verifier a review cycle starts, a new project coordinator, and each PR keeper model run. A cycle queued for a verification slot SHALL resolve its verifiers when it starts, not when it queues; which verifiers it skips SHALL be decided when it is created. It SHALL use the small-work line when the coordinator asked for small work and the row has one, and the main line otherwise. It SHALL pick the first choice unless it is spent, then the then choice unless it is spent, leaving out choices whose provider cannot be chosen; with none left, the start SHALL be refused, naming why. When every remaining choice is spent, it SHALL pin the one whose reset comes earliest, an unknown reset counting as last, and the agent SHALL wait for it. Only a verifier a review cycle starts MAY skip: when its line's then is skip and its first choice is spent or its provider cannot be chosen, it SHALL start nothing and the cycle SHALL record it as skipped (see ticket-verification). A verifier that still has an open finding on the ticket SHALL NOT skip and SHALL wait instead, and so SHALL a reviewer started with `assign_ticket` on a skip row. A running agent SHALL NEVER switch provider, and a started provider conversation SHALL keep its provider. Wiffletree SHALL record on the agent its profile, the chooser Wiffletree, the configuration revision and a plain reason, and log `model_selected`.

#### Scenario: First choice spent
- **WHEN** Claude's weekly window is at 96%, the threshold is 95 and the Implementer row is Opus · medium then GPT 6.1 Sol · high
- **THEN** a new implementer starts on GPT 6.1 Sol · high with the reason "Started on Codex: Claude's weekly limit was at 96% (moves on at 95%). First choice was Opus · medium."

#### Scenario: A running agent is not switched
- **WHEN** a Claude implementer is working when Claude's weekly window reaches 96%
- **THEN** its next turns stay on its Claude model

#### Scenario: First choice available
- **WHEN** Claude reads below the threshold or has no reading
- **THEN** a new implementer starts on its first choice with the reason "First choice"

#### Scenario: Small work
- **WHEN** the coordinator asks for small work and the Implementer row's small line is Sonnet · high then GPT 6.1 Sol · medium
- **THEN** the implementer starts on Sonnet · high with the reason "First choice for small work"

#### Scenario: Small work without a small line
- **WHEN** the coordinator asks for small work and the Tester row has no small line
- **THEN** the tester resolves its main line

#### Scenario: Signed out
- **WHEN** detection found Claude signed out and the Coordinator row is Opus · high then GPT 6.1 Sol · high
- **THEN** a new project's coordinator starts on GPT 6.1 Sol · high, saying Claude is not signed in on this Mac

#### Scenario: A reviewer whose row says skip
- **WHEN** Claude's weekly window is at 96%, the threshold is 95, and a cycle starts the reviewer Correctness, whose row is Opus · high then skip
- **THEN** no Correctness session starts and the cycle records Correctness as skipped with the reason "Claude's weekly limit was at 96%; its row says skip"

#### Scenario: An open finding waits instead of skipping
- **WHEN** Correctness still has an open finding on the ticket, its row says skip and Claude is spent
- **THEN** the next cycle pins Correctness to Opus · high and holds it until the reset

### Requirement: Waiting for the earliest reset
An agent pinned while every choice was spent SHALL wait: at each of its turn starts while its provider is still spent, the host SHALL keep the input queued, set the agent Blocked with a reason naming the provider, the window, its use, the threshold and the reset, and tell its parent once per held input with a message from Wiffletree, not as the agent's report. The host SHALL wake at the reset and re-check on every new quota reading. Once the provider is not spent, the turn SHALL start on the pinned model and the agent SHALL no longer wait. Other agents' work SHALL continue meanwhile. Waiting SHALL be the default; a verifier whose row says skip is left out of the review instead, as resolution says.

#### Scenario: The Claude reviewer waits
- **WHEN** Claude is spent and a cycle starts the reviewer Correctness, whose row is Opus · high then wait
- **THEN** Correctness is pinned to Opus · high and held with the reset time, the cycle's other verifiers run, no verdict is recorded for Correctness, and it reviews after the reset

#### Scenario: Every choice spent
- **WHEN** Claude resets Monday and Codex resets Thursday, both spent, and an implementer is assigned
- **THEN** it is pinned to its Claude choice, held until Monday with its input kept, and the coordinator gets one message saying so

#### Scenario: The Claude reviewer's row says skip
- **WHEN** Claude is spent and a cycle starts the reviewer Correctness, whose row is Opus · high then skip, beside Tests and Regressions on Codex
- **THEN** Correctness is not started or held, Tests and Regressions run, and the cycle can pass without Correctness
