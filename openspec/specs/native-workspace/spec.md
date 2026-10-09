# native-workspace Specification

## Purpose
Render a responsive native chat-first workspace backed by durable host records and explicit prototype capabilities.

## Requirements

### Requirement: Native durable workspace
The macOS client SHALL render project → ticket → agents, select conversations, queue messages, retain per-session drafts during navigation, add tickets and agents, show status separately from attention and open context panels without replacing chat.

A persistent right-edge inspector rail SHALL follow the selected project, ticket or agent. Its destinations SHALL reflect that scope, and its panel SHALL identify the owner. Project Inbox SHALL remain separate from ticket and agent status; Git SHALL appear only for repository-backed ticket and agent views. Ticket, agent, project and repository creation SHALL use separate dialogs with explicit repository or ticket ownership, retained values on failure and guarded pending submissions. Successful project creation SHALL select the new project orchestrator. The hierarchy and context panes SHALL have draggable width dividers and compact native typography.

#### Scenario: Creation fails without losing input
- **WHEN** a repository attachment or session creation fails validation or host persistence
- **THEN** the dialog remains open with its entered values and error, and permits a corrected submission

#### Scenario: Longer ticket names need more space
- **WHEN** the user drags the hierarchy divider wider
- **THEN** the hierarchy gains width while the conversation and optional context panel remain usable

#### Scenario: Change conversation
- **WHEN** the user types a draft, selects a worker and returns
- **THEN** the draft is retained and only bounded transcript pages are rendered

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

### Requirement: Bounded rendering and background IO
The UI SHALL use virtualized transcript rows, consume canonical Tidal semantic colors, offer System/Light/Dark without losing drafts, and perform persistence and Git reads off the UI thread. Idle rendering SHALL use no animation or frame timer; the only recurring work is a one-minute quota check and an hourly update check.

#### Scenario: Large transcript
- **WHEN** a session has 100,000 historical messages
- **THEN** navigation loads a bounded indexed page instead of its full history

### Requirement: Performance evidence
The implementation SHALL ship a repeatable release host benchmark for persisted fixtures and a report distinguishing measured host overhead from unmeasured UI, provider and soak budgets.

#### Scenario: Performance report
- **WHEN** benchmark results are recorded
- **THEN** fixture, hardware, build mode, durability and p95 timings accompany the results

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

### Requirement: Check-in thresholds in Settings
The Settings window SHALL have a Check-ins section under the workspace folder with two preset selectors that save at once. "Check in on an agent every" SHALL offer 10, 25, 50, 100 and 250, followed by "turns", with the caption "The coordinator hears about an agent that has worked this many turns without a message from it. Nothing pauses." "Check in with you every" SHALL offer 50, 100, 250, 500 and 1000, followed by "coordinator turns", with the caption "You get an inbox note when the coordinator has worked this many turns without hearing from you. Nothing pauses." The saved value SHALL be highlighted; a saved value outside the presets SHALL highlight none. A refused save SHALL show the host's error in the section and keep the saved values.

#### Scenario: Choose an agent threshold
- **WHEN** the human clicks 50 beside "Check in on an agent every"
- **THEN** the host saves `checkin_child_turns` 50 with `checkin_human_turns` unchanged, and 50 is highlighted

#### Scenario: Hand-edited value
- **WHEN** `settings.json` holds `checkin_human_turns` 300
- **THEN** the section opens with no coordinator preset highlighted, and the host uses 300
