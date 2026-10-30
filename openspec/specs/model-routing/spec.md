# model-routing Specification

## Purpose
Let the human decide who runs each job on this machine, and let Wiffletree move a job to its next choice when a subscription runs low. Coordinators name jobs, never models.

## Requirements

### Requirement: Runtime model discovery
A read-only Codex feasibility probe SHALL initialize app-server, paginate the model catalog, extract supported efforts and shut down without starting inference.

#### Scenario: Catalog probe
- **WHEN** the probe runs with an installed supported CLI
- **THEN** it reports model capabilities without creating a thread or turn

### Requirement: Model preferences guide
The host SHALL persist a Markdown guide of at most 32 KiB, written by the human, in the shared folder's `settings/model-guide.md` (see settings-repository), so that it is shared with every machine that uses the same settings repository. The guide SHALL be optional notes for coordinators; the host SHALL NOT parse it to choose models. The suggested setup of a machine with no saved model selection SHALL keep a guide that is already in the shared folder rather than suggest a starter guide.

#### Scenario: Guide delivered unchanged
- **WHEN** a coordinator reads its workspace context
- **THEN** it receives the guide exactly as saved as `notes`, with this machine's current revision

#### Scenario: A guide from another machine
- **WHEN** a machine with no saved model selection joins a settings repository that has a guide, and the human opens Models
- **THEN** the suggested setup shows the repository's guide, and saving it keeps that guide

### Requirement: Human model choice wins
The human's model choices SHALL NOT be limited by the table. This covers the new-agent dialog and the chat override. The dialog SHALL be pre-filled with what the role's row would use right now, and SHALL record the chooser as the table when the human keeps it and as the human otherwise. A human's choice SHALL end a wait for a reset. A started conversation SHALL still refuse a provider change.

#### Scenario: Human picks a model outside the role's providers
- **WHEN** the human creates a tester with a Codex model while the Tester row names only Claude
- **THEN** the agent is created with that model, and its selection records the human as the chooser

### Requirement: Held turns wake the parent
When the host holds a turn for a reason other than waiting for a reset, it SHALL deliver a `blocked` report to the agent's parent on the agent's behalf. The report SHALL carry the same text as the session's `last_error` and SHALL wake the parent like any blocked report. Holding again on the same input SHALL NOT send a duplicate. A turn waiting for a reset SHALL tell the parent as "Waiting for the earliest reset" says.

#### Scenario: Parent learns of the hold
- **WHEN** an agent below the project coordinator has queued input but no pinned model
- **THEN** its turn does not start, it is Blocked with an error naming the missing model, and the project coordinator receives one `blocked` report from it and is woken

#### Scenario: Retry without a fix
- **WHEN** the human retries the held agent without giving it a model
- **THEN** it is held again, and the parent receives no second report for the same input

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

### Requirement: No agent before the first saved setup
While no model selection is saved, the host SHALL refuse to create a project, and therefore any agent, with a message that points the human to the Models page. It SHALL NOT fall back to a built-in setup or substitute a provider, model or verifier.

#### Scenario: Project before the first save
- **WHEN** the human creates a project on a store with no saved setup
- **THEN** the host refuses, names the Models page, and creates no project or session

#### Scenario: Project after the first save
- **WHEN** the human saves the suggested setup and creates a project
- **THEN** its coordinator starts on a provider of the saved setup

### Requirement: Welcome on first run
While no model selection is saved, the start screen SHALL show a welcome that leads to the Models page instead of offering a project. Detection SHALL start when the app opens, on every store, so the welcome, the Models page and the host share one result.

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
- **THEN** the start screen reads "Start with a project.", the sidebar is unmarked, and the host learns which CLIs are signed in

### Requirement: Who runs what
The host SHALL persist, per machine, one row per job: Coordinator, Implementer, Tester, one per configured verifier keyed by its focus, and optionally PR keeper. Each row SHALL have a first choice (provider, model, effort), a then choice that is a model, wait (the default, saved as no then) or skip, and an optional small-work line with its own first and then. Skip SHALL be offered only on verifier rows, on either line. A save SHALL be refused, whole, with the previous table kept, when a row of Coordinator, Implementer or Tester is missing or doubled, two rows name one verifier or two PR keeper rows exist, a model or effort is empty or too long, a then choice uses the same provider as its first choice, or a Coordinator, Implementer, Tester or PR keeper line's then is skip. A table saved before skip existed SHALL load unchanged. The table SHALL NOT be shared through the settings repository. A save that also changes the guide or the switch threshold SHALL change all of them or none, and a save together with the review settings SHALL be checked whole before anything is written.

The table SHALL match the configured verifiers whenever it is read: a row for a verifier that is no longer configured SHALL be left out, and a configured verifier without a row SHALL use a copy of the first reviewer's row, or of the Tester row with no reviewer row, and SHALL be named in the table's `copied` list until a save stores its row.

#### Scenario: A then on the same subscription
- **WHEN** the human saves Implementer with Opus · medium then Sonnet · high
- **THEN** the save is refused, saying both choices use the Claude subscription, and the previous table stays

#### Scenario: A verifier from another machine
- **WHEN** a pulled review setting adds the reviewer Security, which has no row on this machine
- **THEN** Security resolves with a copy of the first reviewer's row, the Models page notes it, and saving the page stores the row

#### Scenario: Skip on a row that is not a verifier
- **WHEN** the human saves the Implementer row as Opus · medium then skip
- **THEN** the save is refused, saying only reviews can be skipped, and the previous table stays

### Requirement: Switch threshold
The host SHALL keep one switch threshold, a whole percentage from 50 to 100, default 95, in the shared folder's `settings/models.json`, so every machine with the same settings repository uses it. It SHALL be saved with the Models page's Save.

#### Scenario: A threshold from another machine
- **WHEN** another machine saves a threshold of 90 and this machine pulls it
- **THEN** resolutions on this machine move on at 90%

### Requirement: When a provider counts as spent
A provider SHALL count as spent when its latest quota reading has any window, 5-hour, weekly or any other reported, whose reset has not passed or is unknown, and whose use is at or above the threshold or whose status reports the limit reached. A reading at or above the threshold with an unknown reset SHALL count until a later reading of that window is below the threshold. A provider with no reading, or a reading with no windows, SHALL NOT count as spent. A provider whose CLI the last detection found missing or signed out SHALL NOT be chosen. The host SHALL read each CLI's sign-in status when it starts, without inference and before it handles any request, without holding up its client, and SHALL NOT choose a provider it has not found signed in. The host SHALL use only the providers' own telemetry and SHALL NEVER estimate quota from token counts.

#### Scenario: Over the threshold
- **WHEN** Claude's weekly window reads 96% with a reset next Monday and the threshold is 95
- **THEN** Claude counts as spent until Monday

#### Scenario: Below the threshold or no reading
- **WHEN** Claude's windows read 41% and 80%, or Claude has no reading
- **THEN** Claude does not count as spent

#### Scenario: The reset has passed
- **WHEN** Claude's weekly window read 99% with a reset that has now passed
- **THEN** that window no longer counts

#### Scenario: Unknown reset
- **WHEN** a window reads 97% with no reset time
- **THEN** the provider counts as spent until a reading of that window comes in below the threshold

#### Scenario: Limit reached
- **WHEN** Claude reports a window with the status `rejected` and no percentage
- **THEN** Claude counts as spent until that window resets

### Requirement: Resolution when an agent starts
Wiffletree SHALL resolve a job's row only when an agent starts: an `assign_ticket` that creates an agent, each verifier a review cycle starts, a new project coordinator, and each PR keeper model run. A cycle an earlier host left queued for a verification slot SHALL resolve its verifiers when it starts, not when it queued; which verifiers it skips SHALL be decided when it is created. It SHALL use the small-work line when the coordinator asked for small work and the row has one, and the main line otherwise. It SHALL pick the first choice unless it is spent, then the then choice unless it is spent, leaving out choices whose provider cannot be chosen; with none left, the start SHALL be refused, naming why. When every remaining choice is spent, it SHALL pin the one whose reset comes earliest, an unknown reset counting as last, and the agent SHALL wait for it. Only a verifier a review cycle starts MAY skip: when its line's then is skip and its first choice is spent or its provider cannot be chosen, it SHALL start nothing and the cycle SHALL record it as skipped (see ticket-verification). A verifier that still has an open finding on the ticket SHALL NOT skip and SHALL wait instead, and so SHALL a reviewer started with `assign_ticket` on a skip row. A running agent SHALL NEVER switch provider, and a started provider conversation SHALL keep its provider. Wiffletree SHALL record on the agent its profile, the chooser Wiffletree, the configuration revision and a plain reason, and log `model_selected`.

#### Scenario: First choice spent
- **WHEN** Claude's weekly window is at 96%, the threshold is 95 and the Implementer row is Opus · medium then GPT 6.1 Sol · high
- **THEN** a new implementer starts on GPT 6.1 Sol · high with the reason "Started on Codex: Claude's weekly limit was at 96% (moves on at 95%). First choice was Opus · medium."

#### Scenario: A running agent is not switched
- **WHEN** a Claude implementer is working when Claude's weekly window reaches 96%
- **THEN** its next turns stay on its Claude model

#### Scenario: First choice available
- **WHEN** Claude reads below the threshold or has no reading
- **THEN** a new implementer starts on its first choice with the reason "First choice"

#### Scenario: Small work
- **WHEN** the coordinator asks for small work and the Implementer row's small line is Sonnet · high then GPT 6.1 Sol · medium
- **THEN** the implementer starts on Sonnet · high with the reason "First choice for small work"

#### Scenario: Small work without a small line
- **WHEN** the coordinator asks for small work and the Tester row has no small line
- **THEN** the tester resolves its main line

#### Scenario: Signed out
- **WHEN** detection found Claude signed out and the Coordinator row is Opus · high then GPT 6.1 Sol · high
- **THEN** a new project's coordinator starts on GPT 6.1 Sol · high, saying Claude is not signed in on this Mac

#### Scenario: A reviewer whose row says skip
- **WHEN** Claude's weekly window is at 96%, the threshold is 95, and a cycle starts the reviewer Correctness, whose row is Opus · high then skip
- **THEN** no Correctness session starts and the cycle records Correctness as skipped with the reason "Claude's weekly limit was at 96%; its row says skip"

#### Scenario: An open finding waits instead of skipping
- **WHEN** Correctness still has an open finding on the ticket, its row says skip and Claude is spent
- **THEN** the next cycle pins Correctness to Opus · high and holds it until the reset

### Requirement: Waiting for the earliest reset
An agent pinned while every choice was spent SHALL wait: at each of its turn starts while its provider is still spent, the host SHALL keep the input queued, set the agent Blocked with a reason naming the provider, the window, its use, the threshold and the reset, and tell its parent once per held input with a message from Wiffletree, not as the agent's report. The host SHALL wake at the reset and re-check on every new quota reading. Once the provider is not spent, the turn SHALL start on the pinned model and the agent SHALL no longer wait. Other agents' work SHALL continue meanwhile. Waiting SHALL be the default; a verifier whose row says skip is left out of the review instead, as resolution says.

#### Scenario: The Claude reviewer waits
- **WHEN** Claude is spent and a cycle starts the reviewer Correctness, whose row is Opus · high then wait
- **THEN** Correctness is pinned to Opus · high and held with the reset time, the cycle's other verifiers run, no verdict is recorded for Correctness, and it reviews after the reset

#### Scenario: Every choice spent
- **WHEN** Claude resets Monday and Codex resets Thursday, both spent, and an implementer is assigned
- **THEN** it is pinned to its Claude choice, held until Monday with its input kept, and the coordinator gets one message saying so

#### Scenario: The Claude reviewer's row says skip
- **WHEN** Claude is spent and a cycle starts the reviewer Correctness, whose row is Opus · high then skip, beside Tests and Regressions on Codex
- **THEN** Correctness is not started or held, Tests and Regressions run, and the cycle can pass without Correctness

### Requirement: Who runs what migration
On first open after this release, after the `model-selection` migration, the host SHALL run one idempotent migration recorded in `policy_migrations` as `who-runs-what`. A store with no saved selection SHALL be left alone. Otherwise it SHALL:
- seed the table from the suggested rows on the providers whose CLI is installed and signed in, read once at the upgrade without inference, or on the providers that were enabled when no CLI is signed in, replacing each model with the closest one its provider's CLI lists;
- give each verifier whose saved provider was Claude or Codex that provider's lens row, which waits rather than switching;
- make a saved PR keeper model, or the closest one its CLI lists, the PR keeper row's first choice, and add no PR keeper row when none was saved;
- remove the guide's `## Previous settings (migrated …)` section and keep the rest;
- add a one-time Models page note, "Set up from your previous settings. Check it.", listing everything the table does not hold: every allowed model, a disabled provider's included, the providers per role, the saved PR keeper model, and the removed guide section line by line;
- leave every pinned agent unchanged.

Running it again SHALL change nothing, and the table SHALL survive a restart.

#### Scenario: Michael's machine
- **WHEN** a store with Claude and Codex enabled, the Correctness reviewer on Claude, Regressions on Codex and no PR keeper model is opened
- **THEN** the table is the suggested one without a PR keeper row, Correctness and Regressions wait rather than switch, the guide has no previous-settings section, and the Models page shows the note

#### Scenario: One provider signed in
- **WHEN** only Codex's CLI is signed in at the upgrade
- **THEN** every row uses Codex alone

#### Scenario: A retired model and a disabled provider
- **WHEN** Claude's only allowed model is one its CLI no longer lists, and Codex was disabled though its CLI is signed in
- **THEN** the rows use the closest models the CLIs list, on both providers, and the note lists the retired model and Codex's saved models

#### Scenario: Re-running
- **WHEN** the migrated store is opened again
- **THEN** the table, guide, notes and runtimes are unchanged

### Requirement: Suggested table on first run
A store with no saved model selection SHALL save none on open. The Models page SHALL show a suggested setup as an unsaved draft, built only from providers that are installed and signed in, and SHALL enable Save without further edits when the suggestion is valid. Saving SHALL persist the table and the verifiers exactly as shown, together or not at all. The plan SHALL NOT change the suggestion. Until the first suggestion has arrived, the page SHALL NOT be editable and Save SHALL be disabled.

- **Both providers:** the table is the mockup's: Coordinator Opus · high then GPT 6.1 Sol · high; Implementer Opus · medium then GPT 6.1 Sol · high, small work Sonnet · high then GPT 6.1 Sol · medium; Tester Sonnet · high then GPT 6.1 Sol · medium; Correctness Opus · high then wait; Regressions GPT 6.1 Sol · high then wait; PR keeper Sonnet · medium then GPT 6.1 Sol · medium. The verifiers are a tester, a Claude reviewer and a Codex reviewer. Each model not in the CLI's catalog is replaced by that provider's closest listed model. The guide is the starter guide.
- **One provider:** every row uses it alone, with no then choice. The verifiers are a tester and one reviewer on it. The guide names only it.
- **Neither:** the table is empty, the page says no signed-in CLI was found, and Save is disabled.

Refresh SHALL run detection again. While the draft is still the suggestion as offered, it SHALL follow the new detection; once the human has edited it, their edits SHALL be kept.

#### Scenario: Both signed in
- **WHEN** a fresh store is opened with both CLIs signed in and the human presses Save without editing
- **THEN** nothing was saved before the press
- **AND** the saved table and verifiers are the "both providers" suggestion

#### Scenario: Only one signed in
- **WHEN** a fresh store is opened with only Codex signed in
- **THEN** every row uses Codex with no then choice, the verifiers are a tester and one Codex reviewer, and it can be saved

#### Scenario: Before the first suggestion
- **WHEN** a fresh store is opened and detection has not finished
- **THEN** the page shows "Checking CLI…" with its controls disabled and Save disabled

#### Scenario: Neither signed in
- **WHEN** a fresh store is opened with no signed-in CLI
- **THEN** Save is disabled and the page tells the human to install and sign in to a CLI, then press Refresh

#### Scenario: Refresh after signing in
- **WHEN** the human signs in to Codex and presses Refresh while the Claude-only suggestion is untouched
- **THEN** the draft becomes the "both providers" suggestion

#### Scenario: Refresh after editing
- **WHEN** the human removes a suggested verifier and presses Refresh
- **THEN** the draft keeps their edit

### Requirement: The PR keeper's row
The PR keeper's model SHALL be the table's PR keeper row. With no row, no PR keeper model is chosen. Each PR keeper model run SHALL resolve the row like any agent start. Sorting comments and merging the passages of a skill update that overlap a rewrite SHALL use only the row's Claude choice in its main line, and sorting SHALL wait while Claude is spent, checked again when the comments have been read, which are then read again later; neither SHALL ever run on Codex. Only the human's client SHALL change the row. A PR keeper agent whose resolved provider differs from its own SHALL be replaced by a fresh one, and on the same provider it SHALL be pinned again before its run.

#### Scenario: Nothing chosen
- **WHEN** the table has no PR keeper row
- **THEN** no PR keeper agent is created and the triggers that need a model say to choose one in Models first

#### Scenario: Sorting while Claude is spent
- **WHEN** the PR keeper row is Sonnet · medium then GPT 6.1 Sol · medium and Claude is spent
- **THEN** new comments are not sorted until Claude is no longer spent, and no sorting run starts on Codex

#### Scenario: A row with no Claude choice
- **WHEN** the PR keeper row names only Codex models and a skill update overlaps a rewrite
- **THEN** no model merges it, and the human chooses each wording

### Requirement: A turn that fails on a usage limit is retried at the reset
A turn whose provider fails it after a quota reading during that turn said the provider's limit is reached, a window reported rejected or at 100% whose reset is ahead or unknown, SHALL be a limit failure, whatever the switch threshold. When no such reading came during the turn, the host SHALL decide from the provider's error text, and SHALL record which of the two it used. A turn stopped by the human or its parent, a turn a quit interrupted, a PR keeper's turn and the turn of a session paused at its end SHALL NOT be limit failures.

A limit failure SHALL hold its input and interrupt the session as any failed turn does, and SHALL record on the session's runtime when it failed and when the provider's limit resets, if known. The host SHALL then release the held input as Retry does, with the same note that the turn was interrupted, once the limit has reset: at the known reset, or, when the reset is unknown, at the first quota reading of that provider since the failure that no longer says the limit is reached. It SHALL wake at the reset and look again on every new quota reading. The retried turn SHALL run in the same session on its pinned model. A session paused during the wait SHALL wait for the human. Retry SHALL run the input at once and Skip SHALL drop it, and either SHALL end the wait. The wait SHALL survive a host restart.

The session's parent SHALL be told once, by the turn's result message, that the agent hit its provider's usage limit and is retried automatically at the reset, and that nothing is needed from it; it SHALL NOT be asked to resume the agent. A verifier in a running cycle SHALL keep the cycle open and run in its round again after the retry. While the session waits, its conversation SHALL keep the held-input warning and its Retry and Skip, and SHALL say that it waits for the provider's limit to reset and when it is retried.

#### Scenario: Claude's session limit
- **WHEN** an implementer's Claude turn streams a reading whose 5-hour window is rejected with a reset at 19:00, then fails with "You've hit your session limit"
- **THEN** its input is held, its conversation says it is retried automatically at 19:00, the coordinator gets one message saying nothing is needed, and at 19:00 the input is retried with the interrupted-turn note

#### Scenario: Unknown reset
- **WHEN** a turn fails after a rejected reading with no reset time
- **THEN** it waits until a later reading of that provider no longer says the limit is reached, then it is retried

#### Scenario: Only the error text says so
- **WHEN** a turn fails with an error naming a usage limit and no reading came during the turn
- **THEN** it is a limit failure that waits for the provider's next reading

#### Scenario: Skip during the wait
- **WHEN** the human presses Skip while a limit failure waits
- **THEN** its input is dropped and nothing is retried at the reset

#### Scenario: Another failure
- **WHEN** a turn fails because its provider exited, or the human stops it while the limit is reached
- **THEN** its input is held for a manual Retry or Skip as before

#### Scenario: A verifier hits the limit
- **WHEN** a verifier's turn in a running cycle fails on a usage limit
- **THEN** the cycle stays open, and after the reset the verifier's round message is queued again and its round starts it

#### Scenario: A restart during the wait
- **WHEN** the host restarts while a limit failure waits for a reset
- **THEN** the input is still retried at the reset
