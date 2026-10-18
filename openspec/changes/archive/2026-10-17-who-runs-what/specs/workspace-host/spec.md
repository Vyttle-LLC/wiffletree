## REMOVED Requirements

### Requirement: Assignment tools take an exact model and reason
**Reason**: Coordinators no longer name models; Wiffletree resolves the job's row.
**Migration**: See "Assignment tools ask for a job, not a model".

## ADDED Requirements

### Requirement: Assignment tools ask for a job, not a model
`assign_ticket` SHALL take its ticket, role, instruction and optional focus as before, plus an optional `small` (default false) and, for a reviewer whose focus is not a configured verifier, an optional `like` naming a configured verifier's focus. `verify_ticket` SHALL take its ticket and level, plus an optional `small` that applies to every verifier of the level. Calls that send `profile`, `reason`, `verifiers`, `size`, `complexity` or `provider` SHALL fail, saying that Wiffletree picks the model from Models → Who runs what. The job SHALL be:
- the role's row for a coordinator, implementer or tester;
- a configured verifier's row when the focus names one;
- for an ad-hoc reviewer, the `like` verifier's row, else the first reviewer's row.

A `like` that names no configured verifier SHALL be refused, naming the configured ones, and nothing SHALL be created. A call for an agent that already exists SHALL return it unchanged, without resolving or switching its model. The tool descriptions SHALL say all of this.

#### Scenario: A call without a profile
- **WHEN** the coordinator calls `assign_ticket` for an implementer with only a ticket, role and instruction
- **THEN** the implementer is created on what the Implementer row resolves to, with its reason

#### Scenario: An old profile argument
- **WHEN** the coordinator calls `assign_ticket` with a `profile`
- **THEN** the call fails, saying Wiffletree picks the model, and no agent is created

#### Scenario: An ad-hoc reviewer like a configured one
- **WHEN** the coordinator assigns the reviewer "Security" with `like: "Regressions"`
- **THEN** it resolves the Regressions row

#### Scenario: An unknown like
- **WHEN** the coordinator assigns an ad-hoc reviewer with `like: "Speed"` and no verifier has that focus
- **THEN** the call is refused, naming the configured verifiers, and no agent is created

#### Scenario: Existing assignment is not switched
- **WHEN** the coordinator repeats `assign_ticket` for an existing role and focus with `small: true`
- **THEN** the existing agent is returned with its original profile

## MODIFIED Requirements

### Requirement: The coordinator receives the model configuration
`workspace_context` SHALL give the project coordinator `model_selection`, read fresh on each call: the revision; the switch threshold; each row of the Who runs what table with its job, its main and small lines, and what each line would use right now, as a profile, whether it would wait and the reason; the configured verifiers; the review levels, each with its name, its kind, the focuses of its verifiers and its note on when to use it; the round and cycle caps; and the guide as `notes`. Workers SHALL NOT receive it.

#### Scenario: Configuration edit reaches a running coordinator
- **WHEN** the human changes the Tester row while the project coordinator is idle
- **THEN** its next `workspace_context` call shows the new row and its right-now pick, with the new revision

#### Scenario: Right now follows the limits
- **WHEN** Claude's weekly window reaches 96% and the threshold is 95
- **THEN** the next `workspace_context` shows the Implementer row's right-now pick on its then choice, with the reason

#### Scenario: Review levels reach the coordinator
- **WHEN** the levels are "Full", a full review that runs Tests, Correctness and Regressions, and "Light", a re-check that runs Correctness, and the project coordinator calls `workspace_context`
- **THEN** `model_selection.review_levels` lists both in that order, each with its name, kind, verifiers and note

#### Scenario: A level edit reaches a running coordinator
- **WHEN** the human adds the level "Docs" while the project coordinator is idle
- **THEN** its next `workspace_context` call lists "Docs"

#### Scenario: Workers do not receive the levels
- **WHEN** an implementer calls `workspace_context`
- **THEN** the result has no `model_selection`
