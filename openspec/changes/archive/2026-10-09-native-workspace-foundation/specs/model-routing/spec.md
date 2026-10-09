## Purpose

Let users choose role-specific runtimes and constrain any orchestrator-selected model and reasoning effort.

## ADDED Requirements

### Requirement: Explicit role policies
The host SHALL persist policies for project/task orchestration, implementation, testing, review and maintenance. Each policy SHALL include fixed or bounded automatic mode, default and allowlisted model/effort profiles for small, standard and complex work.

#### Scenario: Fixed role choice
- **WHEN** an implementer has fixed mode and the orchestrator proposes another model
- **THEN** the fixed choice wins and the routing explanation records that fact

### Requirement: Bounded automatic routing
Automatic selection SHALL reject models and efforts outside the user's allowlist, reject provider changes for existing sessions, and validate against a supplied provider catalog. Missing telemetry SHALL remain unknown.

#### Scenario: Out of bounds proposal
- **WHEN** an orchestrator proposes an unallowed model or effort
- **THEN** routing fails visibly without paid fallback or provider switching

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
