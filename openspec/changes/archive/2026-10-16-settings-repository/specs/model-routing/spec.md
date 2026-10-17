## MODIFIED Requirements

### Requirement: Providers on this machine
The host SHALL persist, per machine:
- whether each provider (Claude, Codex) is enabled;
- which model and effort pairs are allowed for it.

A disabled provider SHALL make none of its models available to coordinator choices. Edits SHALL save atomically with role providers, and a save that also changes the guide SHALL change both or neither. A save of the setup together with the review settings SHALL be checked whole before anything is written, and SHALL change the selection, the guide and the review settings all or none. The providers and their models SHALL NOT be shared through the settings repository.

#### Scenario: Codex disabled on this machine
- **WHEN** the human disables Codex and saves
- **THEN** no coordinator can assign a Codex model for any role, including reviewers

#### Scenario: Invalid configuration is not saved
- **WHEN** a save has any of the following:
  - no enabled provider;
  - an enabled provider with no allowed model;
  - a duplicate model and effort pair;
  - a role without an enabled provider
- **THEN** the host rejects the whole save and keeps the previous configuration, the guide included

#### Scenario: A refused combined save
- **WHEN** comment sorting is on with a Claude PR keeper model, and the human saves a changed Review card together with a selection that removes the PR keeper model
- **THEN** the save is refused, and the review settings, the guide and the selection are all as they were

### Requirement: Model preferences guide
The host SHALL persist a Markdown guide of at most 32 KiB, written by the human, in the shared folder's `settings/model-guide.md` (see settings-repository), so that it is shared with every machine that uses the same settings repository. The guide SHALL explain how to choose a model and effort within a provider by the size and risk of the work. The host SHALL NOT parse it to choose or permit models. The suggested setup of a machine with no saved model selection SHALL keep a guide that is already in the shared folder rather than suggest a starter guide.

#### Scenario: Guide delivered unchanged
- **WHEN** a coordinator reads its workspace context
- **THEN** it receives the guide exactly as saved, with this machine's current revision

#### Scenario: A guide from another machine
- **WHEN** a machine with no saved model selection joins a settings repository that has a guide, and the human opens Models
- **THEN** the suggested setup shows the repository's guide, and saving it keeps that guide
