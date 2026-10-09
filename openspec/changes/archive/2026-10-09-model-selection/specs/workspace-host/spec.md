## ADDED Requirements

### Requirement: Assignment tools take an exact model and reason
The `assign_ticket` MCP tool SHALL require a `profile` (provider, model, effort) and a one-line `reason` in place of `provider`, `size` and `complexity`. Its other arguments SHALL be unchanged. Calls that still send `size`, `complexity` or `provider` SHALL fail with a message naming the replacement. A call for an agent that already exists SHALL return it unchanged, without checking or switching its model.

#### Scenario: Old size argument
- **WHEN** the coordinator calls `assign_ticket` with `size: "big"`
- **THEN** the call fails, saying to choose an exact profile within the role's providers, and no agent is created

#### Scenario: Existing assignment is not switched
- **WHEN** the coordinator repeats `assign_ticket` for an existing role and focus with a different profile
- **THEN** the existing agent is returned with its original profile

### Requirement: The coordinator receives the model configuration
`workspace_context` SHALL give the project coordinator the enabled providers with their allowed models, the providers per role, the configured verifiers and the guide, read fresh on each call. Workers SHALL NOT receive them.

#### Scenario: Configuration edit reaches a running coordinator
- **WHEN** the human adds Codex to the Implementer role while the project coordinator is idle
- **THEN** its next `workspace_context` call shows Implementer using Claude and Codex, with the new revision
