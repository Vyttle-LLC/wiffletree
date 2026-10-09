## MODIFIED Requirements

### Requirement: Configurable routing and memory
Context panels SHALL show scoped logs, support milestone capture and show persisted activity and attention. Optional Git history SHALL show real read-only data. The Models page SHALL have four cards:
- **Providers:** an enable toggle per provider; the CLI status that existing discovery reports; an Add row with a model dropdown that omits models already listed and an Add button, which allows the model at its first effort; and one line per allowed model with a checkbox for each effort it supports and Remove. Unticking a model's last effort removes it. The allowlist stays a set of exact model and effort pairs.
- **Roles:** provider checkboxes for the project coordinator, implementer and tester.
- **Review:** the verifier list (role, focus, provider or "Any", instruction), the round cap and the cycle cap, with "Run reviso:style with Codex" as example text. Each change saves at once. When the list needs more turns than a project's limit allows, the card notes it, for example "4 verifiers need a project turn limit of at least 5"; it never changes a limit. Settings SHALL NOT edit the verifier list.
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

#### Scenario: Cycle cap on the Review card
- **WHEN** the human sets the cycle cap to 3 on the Review card
- **THEN** it saves at once, survives restart and appears in the coordinator's next workspace context
