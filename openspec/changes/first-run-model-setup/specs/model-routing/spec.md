## ADDED Requirements

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

## MODIFIED Requirements

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
