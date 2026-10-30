## MODIFIED Requirements

### Requirement: Resolution when an agent starts
Wiffletree SHALL resolve a job's row only when an agent starts: an `assign_ticket` that creates an agent, each verifier a review cycle starts, a new project coordinator, and each PR keeper model run. A cycle an earlier host left queued for a verification slot SHALL resolve its verifiers when it starts, not when it queued; which verifiers it skips SHALL be decided when it is created. It SHALL use the small-work line when the coordinator asked for small work and the row has one, and the main line otherwise. It SHALL pick the first choice unless it is spent, then the then choice unless it is spent, leaving out choices whose provider cannot be chosen; with none left, the start SHALL be refused, naming why. When every remaining choice is spent, it SHALL pin the one whose reset comes earliest, an unknown reset counting as last, and the agent SHALL wait for it. Only a verifier a review cycle starts MAY skip: when its line's then is skip and its first choice is spent or its provider cannot be chosen, it SHALL start nothing and the cycle SHALL record it as skipped (see ticket-verification). A verifier that still has an open finding on the ticket SHALL NOT skip and SHALL wait instead, and so SHALL a reviewer started with `assign_ticket` on a skip row. A running agent SHALL NEVER switch provider, and a started provider conversation SHALL keep its provider. Wiffletree SHALL record on the agent its profile, the chooser Wiffletree, the configuration revision and a plain reason, and log `model_selected`.

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
