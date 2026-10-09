## MODIFIED Requirements

### Requirement: Configurable routing and memory
Context panels SHALL show scoped logs, support milestone capture and show persisted activity and attention. Optional Git history SHALL show real read-only data. The Models page SHALL have four cards:
- **Providers:** an enable toggle per provider; the CLI status that existing discovery reports; an Add row with a model dropdown that omits models already listed and an Add button, which allows the model at its first effort; and one line per allowed model with a checkbox for each effort it supports and Remove. Unticking a model's last effort removes it. The allowlist stays a set of exact model and effort pairs.
- **Roles:** provider checkboxes for the project coordinator, implementer and tester.
- **Review:** the verifier list (role, focus, provider or "Any", instruction), the round cap and the cycle cap, with "Run reviso:style with Codex" as example text. The round cap SHALL be labelled "Rounds per review", with the caption "The first review plus re-checks after the implementer's fixes. If the last round still finds blocking problems, the review stops and the coordinator decides." The cycle cap SHALL be labelled "Reviews per ticket", with the caption "Full reviews with fresh reviewers. After the last one, the coordinator accepts, accepts with a waiver, closes the ticket or asks you." Each change saves at once. The card SHALL NOT warn about turn limits, because there are none. Settings SHALL NOT edit the verifier list.
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

#### Scenario: Caps in plain words
- **WHEN** the human opens the Review card
- **THEN** it shows "Rounds per review" and "Reviews per ticket", each with its caption, and no control is labelled "Round cap" or "Cycle cap"

#### Scenario: Long verifier list
- **WHEN** the human adds a seventh verifier on the Review card
- **THEN** it saves at once and the card shows no turn-limit note

## ADDED Requirements

### Requirement: Check-in thresholds in Settings
The Settings window SHALL have a Check-ins section under the workspace folder with two preset selectors that save at once. "Check in on an agent every" SHALL offer 10, 25, 50, 100 and 250, followed by "turns", with the caption "The coordinator hears about an agent that has worked this many turns without a message from it. Nothing pauses." "Check in with you every" SHALL offer 50, 100, 250, 500 and 1000, followed by "coordinator turns", with the caption "You get an inbox note when the coordinator has worked this many turns without hearing from you. Nothing pauses." The saved value SHALL be highlighted; a saved value outside the presets SHALL highlight none. A refused save SHALL show the host's error in the section and keep the saved values.

#### Scenario: Choose an agent threshold
- **WHEN** the human clicks 50 beside "Check in on an agent every"
- **THEN** the host saves `checkin_child_turns` 50 with `checkin_human_turns` unchanged, and 50 is highlighted

#### Scenario: Hand-edited value
- **WHEN** `settings.json` holds `checkin_human_turns` 300
- **THEN** the section opens with no coordinator preset highlighted, and the host uses 300
