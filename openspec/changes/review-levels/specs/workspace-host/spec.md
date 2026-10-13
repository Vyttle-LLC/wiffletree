## MODIFIED Requirements

### Requirement: The coordinator receives the model configuration
`workspace_context` SHALL give the project coordinator the enabled providers with their allowed models, the providers per role, the configured verifiers, the review levels, each with its name, its kind, the focuses of its verifiers and its note on when to use it, and the guide, read fresh on each call. Workers SHALL NOT receive them.

#### Scenario: Configuration edit reaches a running coordinator
- **WHEN** the human adds Codex to the Implementer role while the project coordinator is idle
- **THEN** its next `workspace_context` call shows Implementer using Claude and Codex, with the new revision

#### Scenario: Review levels reach the coordinator
- **WHEN** the levels are "Full", a full review that runs Tests, Correctness and Regressions, and "Light", a re-check that runs Correctness, and the project coordinator calls `workspace_context`
- **THEN** `model_selection.review_levels` lists both in that order, each with its name, kind, verifiers and note

#### Scenario: A level edit reaches a running coordinator
- **WHEN** the human adds the level "Docs" while the project coordinator is idle
- **THEN** its next `workspace_context` call lists "Docs"

#### Scenario: Workers do not receive the levels
- **WHEN** an implementer calls `workspace_context`
- **THEN** the result has no `model_selection`
