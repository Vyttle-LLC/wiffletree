# model-routing Specification

## Purpose
Let the human decide which providers and models run on this machine, and make coordinators choose exact models within those limits.

## Requirements

### Requirement: Runtime model discovery
A read-only Codex feasibility probe SHALL initialize app-server, paginate the model catalog, extract supported efforts and shut down without starting inference.

#### Scenario: Catalog probe
- **WHEN** the probe runs with an installed supported CLI
- **THEN** it reports model capabilities without creating a thread or turn

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

### Requirement: Providers per role
The host SHALL persist a non-empty set of enabled providers for each of the project coordinator, implementer and tester roles. A coordinator's choice for these roles SHALL use one of the role's providers. A configured verifier's provider limits its choice; an ad-hoc reviewer may use any enabled provider.

#### Scenario: Two providers allowed for implementers
- **WHEN** the Implementer role lists Claude and Codex
- **THEN** a coordinator may assign an implementer any allowed model from either provider

### Requirement: Model preferences guide
The host SHALL persist a Markdown guide of at most 32 KiB, written by the human, in the shared folder's `settings/model-guide.md` (see settings-repository), so that it is shared with every machine that uses the same settings repository. The guide SHALL explain how to choose a model and effort within a provider by the size and risk of the work. The host SHALL NOT parse it to choose or permit models. The suggested setup of a machine with no saved model selection SHALL keep a guide that is already in the shared folder rather than suggest a starter guide.

#### Scenario: Guide delivered unchanged
- **WHEN** a coordinator reads its workspace context
- **THEN** it receives the guide exactly as saved, with this machine's current revision

#### Scenario: A guide from another machine
- **WHEN** a machine with no saved model selection joins a settings repository that has a guide, and the human opens Models
- **THEN** the suggested setup shows the repository's guide, and saving it keeps that guide

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

Running it again SHALL change nothing. A store with no saved policy SHALL get no model selection from the migration. A store that already has a saved model selection SHALL be unchanged, byte for byte, by opening it and by opening the Models page.

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

#### Scenario: Fresh store
- **WHEN** a store with no saved policy is opened for the first time
- **THEN** the migration is recorded and no model selection or settings file is saved

### Requirement: The PR keeper's model
The saved model selection SHALL hold at most one exact model for the PR keeper: a provider, a model and an effort. None SHALL be chosen until the human chooses one, also after an update, on a fresh install and in the suggested setup of a machine with no saved selection. A save SHALL be refused, whole, when the keeper's model is not an allowed model of an enabled provider, and the previous selection SHALL stay in effect. The Models page SHALL show a "PR keeper" card with that one choice, saying what the model is used for, that watching and rebasing use no model, and that a Claude model also merges the passages of a skill update that overlap a rewrite. Only the human's client SHALL change it. A PR keeper agent SHALL be pinned to the model saved when it is created, and the host SHALL never run the keeper, or a skill merge, on another model.

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

#### Scenario: A Codex keeper model and a skill update
- **WHEN** the PR keeper's model is a Codex model and a skill update overlaps a rewrite
- **THEN** no model merges it, and the card says skill merges need a Claude model

### Requirement: Provider detection
For each provider the app SHALL detect whether its CLI is installed, whether it is signed in, and its plan when known, without starting inference. Detection SHALL run off the UI thread and SHALL treat a missing CLI, one that fails, one whose output cannot be read and one that does not answer within 10 seconds as not signed in. The plan SHALL be best effort: any failure to read it means the plan is unknown. Token contents SHALL NOT be logged or shown. The Models page SHALL show each provider's status, and SHALL show the plan beside it when known.

#### Scenario: Signed in with a plan
- **WHEN** `claude auth status --json` reports `loggedIn: true` and `subscriptionType: "max"`
- **THEN** the Models page shows Claude as "CLI found · signed in · Max plan"

#### Scenario: Installed but signed out
- **WHEN** the Codex CLI is installed and `codex login status` does not report "Logged in"
- **THEN** Codex is shown as "CLI found · not signed in" and is not used by a suggested setup

#### Scenario: Unreadable Codex plan
- **WHEN** Codex is signed in and `~/.codex/auth.json` is missing, unreadable or malformed
- **THEN** Codex is shown as signed in with no plan, and nothing fails

### Requirement: Suggested setup on first run
A store with no saved model selection SHALL save none on open. The Models page SHALL show a suggested setup as an unsaved draft, built only from providers that are installed and signed in, and SHALL enable Save without further edits when the suggestion is valid. Saving SHALL persist the model selection and the verifiers exactly as shown, together or not at all. The plan SHALL NOT change the suggestion. Until the first suggestion has arrived, the page SHALL NOT be editable and Save SHALL be disabled. A provider SHALL carry the Suggested tag only while it is enabled in the draft.

- **Both providers:** Claude allows Opus and Sonnet at high and medium; Codex allows GPT 6.1 Sol at high and medium. The project coordinator and implementer use Claude and the tester uses Codex. The verifiers are a tester, a Claude reviewer and a Codex reviewer. The guide is the starter guide.
- **One provider:** it is enabled with its models above and takes every role. The verifiers are a tester and one reviewer on it. The guide names only it. The other provider is disabled and shows a one-line fix.
- **Neither:** both are disabled, the page says no signed-in CLI was found, and Save is disabled.

Refresh SHALL run detection again. While the draft is still the suggestion as offered, it SHALL follow the new detection; once the human has edited it, their edits SHALL be kept.

#### Scenario: Both signed in
- **WHEN** a fresh store is opened with both CLIs signed in and the human presses Save without editing
- **THEN** nothing was saved before the press
- **AND** the saved selection and verifiers are the "both providers" suggestion

#### Scenario: Only one signed in
- **WHEN** a fresh store is opened with only Codex signed in
- **THEN** the suggestion enables Codex alone, gives it every role, lists a tester and one Codex reviewer, and can be saved

#### Scenario: Neither signed in
- **WHEN** a fresh store is opened with no signed-in CLI
- **THEN** Save is disabled and the page tells the human to install and sign in to a CLI, then press Refresh

#### Scenario: Before the first suggestion
- **WHEN** a fresh store is opened and detection has not finished
- **THEN** the page shows "Checking CLI…" with its controls disabled and Save disabled

#### Scenario: Suggested provider switched off
- **WHEN** the human switches off a suggested provider
- **THEN** it no longer carries the Suggested tag, and switching it back on restores the tag

#### Scenario: Refresh after signing in
- **WHEN** the human signs in to Codex and presses Refresh while the Claude-only suggestion is untouched
- **THEN** the draft becomes the "both providers" suggestion

#### Scenario: Refresh after editing
- **WHEN** the human removes a suggested verifier and presses Refresh
- **THEN** the draft keeps their edit

### Requirement: No agent before the first saved setup
While no model selection is saved, the host SHALL refuse to create a project, and therefore any agent, with a message that points the human to the Models page. It SHALL NOT fall back to a built-in setup or substitute a provider, model or verifier.

#### Scenario: Project before the first save
- **WHEN** the human creates a project on a store with no saved setup
- **THEN** the host refuses, names the Models page, and creates no project or session

#### Scenario: Project after the first save
- **WHEN** the human saves the suggested setup and creates a project
- **THEN** its coordinator starts on a provider of the saved setup

### Requirement: Welcome on first run
While no model selection is saved, the start screen SHALL show a welcome that leads to the Models page instead of offering a project, and detection SHALL start when the app opens so the welcome and the Models page share one result. A store with a saved model selection SHALL NOT show the welcome and SHALL NOT run detection before its Models page is opened.

- **A CLI signed in:** two steps, Set up your models and Start a project; each provider as detected, signed in with its plan when known or with its one-line fix; and a Review suggested setup button that opens the Models page.
- **No CLI signed in:** three steps, Sign in to a CLI, Set up your models and Start a project; each provider's one-line fix; and a Check again button that runs detection again.
- **Detection not finished:** a checking state. The no-CLI welcome SHALL NOT show before every provider has a result.

A fix on the welcome SHALL NOT name a control the welcome does not have: a signed-out CLI's fix says to run its sign-in command, without the Models page's Refresh.

While no model selection is saved, New project SHALL NOT be offered: the start screen has no New project button, the sidebar's New project button is dimmed and says it is available once models are set up, and that button and the New Project shortcut SHALL open the Models page without asking the host to create a project. The sidebar SHALL say to set up models to create the first project and SHALL mark its Models entry. After the first save, without a restart, the start screen SHALL offer a project and New project SHALL work everywhere.

#### Scenario: Both signed in
- **WHEN** a fresh store is opened with both CLIs signed in
- **THEN** the start screen welcomes the human, lists both providers with their plans, and Review suggested setup opens the Models page with the suggestion

#### Scenario: No CLI signed in
- **WHEN** a fresh store is opened with no signed-in CLI
- **THEN** the welcome shows the three steps and each provider's fix
- **AND** after the human signs in and presses Check again, it shows the two steps with that provider found

#### Scenario: Detection still running
- **WHEN** a fresh store is opened and a CLI has not answered yet
- **THEN** the start screen shows that it is checking the CLIs, not the no-CLI welcome

#### Scenario: New project before the first save
- **WHEN** the human presses the New Project shortcut or the sidebar's New project button on a store with no saved setup
- **THEN** the Models page opens, no project is requested and no error is shown

#### Scenario: After the first save
- **WHEN** the human saves the suggested setup and closes the Models page
- **THEN** the start screen reads "Start with a project." and New project creates a project

#### Scenario: Saved store
- **WHEN** a store with a saved setup and no projects is opened
- **THEN** the start screen reads "Start with a project.", the sidebar is unmarked and no CLI status command has run
