## Purpose

Render a responsive native chat-first workspace backed by durable host records and explicit prototype capabilities.

## ADDED Requirements

### Requirement: Native durable workspace
The macOS client SHALL render project → task → agents, select conversations, queue messages, retain per-session drafts during navigation, add tasks and workers, show status separately from attention and open context panels without replacing chat.

A persistent right-edge inspector rail SHALL follow the selected project, task or agent. Its destinations SHALL reflect that scope, and its panel SHALL identify the owner. Project Inbox SHALL remain separate from task/worker status; Git SHALL appear only for repository-backed task/worker views. Task, agent, project and repository creation SHALL use separate dialogs with explicit repository or task ownership, retained values on failure and guarded pending submissions. Successful project creation SHALL select the new project orchestrator. The hierarchy and context panes SHALL have draggable width dividers and compact native typography.

#### Scenario: Creation fails without losing input
- **WHEN** a repository attachment or session creation fails validation or host persistence
- **THEN** the dialog remains open with its entered values and error, and permits a corrected submission

#### Scenario: Longer task names need more space
- **WHEN** the user drags the hierarchy divider wider
- **THEN** the hierarchy gains width while the conversation and optional context panel remain usable

#### Scenario: Change conversation
- **WHEN** the user types a draft, selects a worker and returns
- **THEN** the draft is retained and only bounded transcript pages are rendered

### Requirement: Configurable routing and memory
Context panels SHALL display and edit role policies, explain allowlisted automatic choices, show scoped logs, support milestone capture and show persisted activity and attention. Optional Git history SHALL show real read-only data.

#### Scenario: Unavailable integration
- **WHEN** a user opens provider quota, semantic compilation or PR integration
- **THEN** the UI identifies unavailable capability instead of presenting sample data as live

### Requirement: Bounded rendering and background IO
The UI SHALL use virtualized transcript rows, consume canonical Tidal semantic colors, offer System/Light/Dark without losing drafts, and perform persistence and Git reads off the UI thread. Idle rendering SHALL use no recurring application timer.

#### Scenario: Large transcript
- **WHEN** a session has 100,000 historical messages
- **THEN** navigation loads a bounded indexed page instead of its full history

### Requirement: Performance evidence
The implementation SHALL ship a repeatable release host benchmark for persisted fixtures and a report distinguishing measured host overhead from unmeasured UI, provider and soak budgets.

#### Scenario: Performance report
- **WHEN** benchmark results are recorded
- **THEN** fixture, hardware, build mode, durability and p95 timings accompany the results
