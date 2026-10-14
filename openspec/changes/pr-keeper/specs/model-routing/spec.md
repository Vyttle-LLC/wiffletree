## ADDED Requirements

### Requirement: The PR keeper's model
The saved model selection SHALL hold at most one exact model for the PR keeper: a provider, a model and an effort. None SHALL be chosen until the human chooses one, also after an update, on a fresh install and in the suggested setup of a machine with no saved selection. A save SHALL be refused, whole, when the keeper's model is not an allowed model of an enabled provider, and the previous selection SHALL stay in effect. The Models page SHALL show a "PR keeper" card with that one choice, saying what the model is used for and that watching and rebasing use no model. Only the human's client SHALL change it. A PR keeper agent SHALL be pinned to the model saved when it is created, and the host SHALL never run the keeper on another model.

#### Scenario: Nothing chosen at first
- **WHEN** the app is updated or first started
- **THEN** no PR keeper model is chosen, and the card says so

#### Scenario: Choosing a model
- **WHEN** the human picks Claude · haiku · medium, an allowed model, and saves
- **THEN** the selection holds it, and the next PR keeper agent is pinned to it

#### Scenario: A model that is not allowed
- **WHEN** a save names a PR keeper model whose provider is disabled or that is not among its allowed models
- **THEN** the whole save is refused and the previous selection stays in effect

#### Scenario: Removing the model from the allowed list
- **WHEN** the human removes the keeper's model from the allowed models without choosing another for the keeper
- **THEN** the save is refused, naming the PR keeper

#### Scenario: No fallback
- **WHEN** the keeper's model cannot run because its provider is signed out
- **THEN** the run fails and no other model is used
