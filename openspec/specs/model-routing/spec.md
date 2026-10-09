# model-routing Specification

## Purpose
Let the human decide which providers and models run on this machine, and make coordinators choose exact models within those limits.

## Requirements

### Requirement: Bounded turn concurrency
Active reasoning SHALL respect global and project limits and release capacity after completion or failure. Inspection and attention handling SHALL not consume reasoning slots.

#### Scenario: Project exhausted
- **WHEN** one project is at its limit while another has capacity
- **THEN** the former waits and the latter remains eligible within the global limit

### Requirement: Runtime model discovery
A read-only Codex feasibility probe SHALL initialize app-server, paginate the model catalog, extract supported efforts and shut down without starting inference.

#### Scenario: Catalog probe
- **WHEN** the probe runs with an installed supported CLI
- **THEN** it reports model capabilities without creating a thread or turn

### Requirement: Providers on this machine
The host SHALL persist, per machine:
- whether each provider (Claude, Codex) is enabled;
- which model and effort pairs are allowed for it.

A disabled provider SHALL make none of its models available to coordinator choices. Edits SHALL save atomically with role providers and the guide.

#### Scenario: Codex disabled on this machine
- **WHEN** the human disables Codex and saves
- **THEN** no coordinator can assign a Codex model for any role, including reviewers

#### Scenario: Invalid configuration is not saved
- **WHEN** a save has any of the following:
  - no enabled provider;
  - an enabled provider with no allowed model;
  - a duplicate model and effort pair;
  - a role without an enabled provider
- **THEN** the host rejects the whole save and keeps the previous configuration

### Requirement: Providers per role
The host SHALL persist a non-empty set of enabled providers for each of the project coordinator, implementer and tester roles. A coordinator's choice for these roles SHALL use one of the role's providers. A configured verifier's provider limits its choice; an ad-hoc reviewer may use any enabled provider.

#### Scenario: Two providers allowed for implementers
- **WHEN** the Implementer role lists Claude and Codex
- **THEN** a coordinator may assign an implementer any allowed model from either provider

### Requirement: Model preferences guide
The host SHALL persist a Markdown guide of at most 32 KiB, written by the human. The guide SHALL explain how to choose a model and effort within a provider by the size and risk of the work. The host SHALL NOT parse it to choose or permit models.

#### Scenario: Guide delivered unchanged
- **WHEN** a coordinator reads its workspace context
- **THEN** it receives the guide exactly as saved, with the current revision

### Requirement: Coordinator selection within providers
A coordinator assigning an agent or starting a verifier SHALL name an exact provider, model and effort and a one-line reason. The host SHALL reject the choice in any of these cases:
- the provider is disabled, or the model and effort are not allowed for that provider;
- for roles other than reviewer, the provider is not configured for the assigned role;
- for a configured verifier that names a provider, the choice uses another provider.

The host SHALL reject without fallback, substitution or provider switch. On success it SHALL record the profile, reason, chooser and configuration revision on the new session.

#### Scenario: Allowed choice is recorded
- **WHEN** the project coordinator assigns an implementer Claude · sonnet · high with the reason "Small, contained CLI flag; Sonnet is enough"
- **THEN** the implementer is pinned to that profile, and its selection records the coordinator, the reason and the revision

#### Scenario: Wrong provider for the role
- **WHEN** a coordinator assigns an implementer Codex · gpt-6.1-sol · high while Implementer uses only Claude
- **THEN** the call fails with "Codex is not configured for Implementer on this machine"
- **AND** no session is created and a `model_rejected` activity is recorded

#### Scenario: Model not allowed
- **WHEN** a coordinator assigns Claude · opus · max and that pair is not allowed
- **THEN** the call fails, listing the allowed Claude models
- **AND** no session is created and no other model is used

#### Scenario: Assignment is all or nothing
- **WHEN** recording the chosen model fails while a coordinator assigns an agent
- **THEN** no session, instruction, ticket change or `model_selected` event remains, and a retry creates the agent with its model

#### Scenario: Missing reason
- **WHEN** a coordinator assigns an allowed profile with an empty or multi-line reason
- **THEN** the call fails and no session is created

### Requirement: Human model choice wins
The human's model choices SHALL NOT be limited by providers, role providers, verifier providers or allowed models. This covers the new-agent dialog and the chat override. Pickers SHALL be pre-filled as follows:
- the role's first configured provider, in the order Claude then Codex;
- for a reviewer, or a role whose providers have no allowed model, the first enabled provider with an allowed model;
- then that provider's first allowed model.

A started conversation SHALL still refuse a provider change.

#### Scenario: Human picks a model outside the role's providers
- **WHEN** the human creates a tester with a Codex model while Tester uses only Claude
- **THEN** the agent is created with that model, and its selection records the human as the chooser

### Requirement: Held turns for disabled providers
At each turn start of a coordinator-chosen session, the host SHALL check that the session's provider is still enabled on this machine. If it is not, the host SHALL:
- not start the turn;
- keep the input queued;
- set the session Blocked with an error naming the provider;
- never switch models.

Human-chosen sessions SHALL NOT be held.

#### Scenario: Provider disabled after assignment
- **WHEN** the human disables Codex while a coordinator-chosen Codex reviewer has queued input
- **THEN** the reviewer's turn does not start, and it is Blocked with "Codex is disabled on this machine"

#### Scenario: Agent without a pinned model
- **WHEN** an agent below the project coordinator has queued input but no pinned model
- **THEN** its turn does not start, it is Blocked with an error naming the missing model, and no other model is used

#### Scenario: Skip a held turn
- **WHEN** the human chooses Skip on a held agent
- **THEN** the input it was holding is cancelled and does not run later

### Requirement: Held turns wake the parent
When the host holds a turn, it SHALL deliver a `blocked` report to the agent's parent on the agent's behalf. The report SHALL carry the same text as the session's `last_error` and SHALL wake the parent like any blocked report. Holding again on the same input SHALL NOT send a duplicate.

#### Scenario: Parent learns of the hold
- **WHEN** a Codex reviewer assigned by the project coordinator is held because Codex was disabled
- **THEN** the project coordinator receives a `blocked` report from the reviewer naming the disabled provider, and is woken

#### Scenario: Retry without a fix
- **WHEN** the human retries the held reviewer without re-enabling Codex
- **THEN** the reviewer is held again, and the coordinator receives no second report for the same input

### Requirement: Model selection migration
On first open after upgrade, after store schema 6 has retired repository coordinators, the host SHALL run one idempotent migration recorded in `policy_migrations`. It SHALL handle every saved policy shape:
- the legacy fixed Codex and maintenance shapes;
- fixed and automatic modes;
- Big/Small provider profiles;
- the repository coordinator row;
- rows with or without a former turn budget.

The migration SHALL:
- **Allowed models:** union all roles' allowed profiles, grouped by provider. Each provider that has any allowed model is enabled.
- **Role providers:** set the project coordinator, implementer and tester's providers to each role's previous default provider.
- **Starter guide:** record each role's Big/Small sizing per provider, its tiers and its fixed mode as prose, and each saved verifier size as a line.
- **Sessions:** first pin every unpinned session to the profile it would have used.
- **Legacy rows:** leave the saved policy rows as they were.

Running it again SHALL change nothing.

#### Scenario: Current Big/Small data
- **WHEN** a store is opened in which every role uses Claude by default, except the reviewer, which defaults to Codex with Big/Small profiles for both providers
- **THEN** the project coordinator, the implementer and the tester use Claude only
- **AND** every previously allowed profile is allowed under its provider
- **AND** the guide states each role's previous large-work and small-work models per provider

#### Scenario: Legacy fixed maintenance policy
- **WHEN** the maintenance role still has the legacy fixed Codex policy
- **THEN** its allowed profiles join the Codex allowlist, and the guide records that it was fixed and lists its tiers

#### Scenario: Pre-flatten store
- **WHEN** a schema-5 store with a repository coordinator and legacy policies, including its row, is opened
- **THEN** schema 6 retires the repository coordinator first, and the model selection has no repository-coordinator role but keeps that row's models allowed and its settings in the guide

#### Scenario: Unpinned session keeps its model
- **WHEN** an implementer that has never run has no pinned profile and its policy was automatic with Big/Small
- **THEN** it is pinned to its provider's previous Big profile 

#### Scenario: Re-running is a no-op
- **WHEN** a migrated store is opened again after the human has edited its configuration
- **THEN** the providers, role providers, guide, session runtimes and policy rows are unchanged
