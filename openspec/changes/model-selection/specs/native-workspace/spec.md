## MODIFIED Requirements

### Requirement: Configurable routing and memory
Context panels SHALL show scoped logs, support milestone capture and show persisted activity and attention. Optional Git history SHALL show real read-only data. The Models page SHALL have four cards:
- **Providers:** an enable toggle per provider; the CLI status that existing discovery reports; an Add row with a model dropdown that omits models already listed and an Add button, which allows the model at its first effort; and one line per allowed model with a checkbox for each effort it supports and Remove. Unticking a model's last effort removes it. The allowlist stays a set of exact model and effort pairs.
- **Roles:** provider checkboxes for the project coordinator, implementer and tester.
- **Review:** the verifier list (role, focus, provider or "Any", instruction) and the round cap, with "Run reviso:style with Codex" as example text. Each change saves at once. When the list needs more turns than a project's limit allows, the card notes it, for example "4 verifiers need a project turn limit of at least 5"; it never changes a limit. Settings SHALL NOT edit the verifier list.
- **Guide:** a Markdown editor.

Providers, Roles and Guide SHALL save together with one Save.

The page SHALL NOT offer routing tiers, Big/Small profiles or per-role default models.

#### Scenario: Unavailable integration
- **WHEN** a user opens provider quota, semantic compilation or PR integration
- **THEN** the UI identifies unavailable capability instead of presenting sample data as live

#### Scenario: Configure a single-subscription machine
- **WHEN** the human disables Claude, sets every role to Codex, edits the guide and saves
- **THEN** all of it survives restart and appears in the coordinator's next workspace context

#### Scenario: Role left without a provider
- **WHEN** the human disables the only provider a role uses
- **THEN** that role shows an inline error and Save stays disabled until it has an enabled provider

## ADDED Requirements

### Requirement: Model picker in creation dialogs
The new-agent dialog SHALL include a model picker. It SHALL be pre-filled with the role's first configured provider and that provider's first allowed model. It SHALL list the role's configured providers' allowed models first and still offer every other catalog model.

#### Scenario: Pre-fill for Reviewer
- **WHEN** the human opens the new-agent dialog for a Reviewer while Claude and Codex are enabled
- **THEN** the picker shows Claude's first allowed model, grouped above Codex's allowed models and the other models

### Requirement: Agents show how their model was chosen
An agent's chat SHALL show, above the composer's model pills, how its model was chosen: by which coordinator and its reason, by the human, or by role default. The Tickets panel SHALL show each assigned agent's model and reason under its ticket. The sidebar SHALL stay unchanged.

#### Scenario: Custom review listed with its focus
- **WHEN** a ticket has Claude, Codex and Style reviewers
- **THEN** its Tickets panel card lists all three, each with its model and reason

#### Scenario: Coordinator reason visible
- **WHEN** the human opens an implementer assigned by the project coordinator
- **THEN** the line above the composer reads "Chosen by ‹coordinator›: ‹reason›", and the ticket's card in the Tickets panel shows the same model and reason

### Requirement: Visible assignment rejection
A coordinator's rejected assignment SHALL appear as a failed step in that coordinator's work stream and as a `model_rejected` entry in Events, carrying the host's message. No agent SHALL appear.

#### Scenario: Wrong provider rejected
- **WHEN** the project coordinator assigns an implementer a Codex model while Implementer uses only Claude
- **THEN** its work stream shows the failed `assign_ticket` step saying Codex is not configured for Implementer, and neither the sidebar nor the Tickets panel shows a new implementer
