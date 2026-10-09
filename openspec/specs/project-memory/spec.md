# project-memory Specification

## Purpose
Preserve meaningful work across replacement sessions and providers using durable scoped records and portable Markdown.

## Requirements

### Requirement: Stable scoped milestones
Logs SHALL carry project, task, session, milestone identity, type, reference, changed, why and state. Repeated identical capture SHALL return the original record; changed payload under the same identity SHALL fail. Retrieval SHALL not expose another project's entries.

#### Scenario: Same day milestones
- **WHEN** a session captures two milestones on one day and repeats the first
- **THEN** both distinct entries remain once and neither is skipped by a date cursor

### Requirement: Safe Markdown export
Maintenance SHALL export only to an explicitly attached brain, preserve existing content and compiler markers, use stable entry markers and recover an interrupted export without duplication. Unsafe repository log filenames SHALL be rejected.

#### Scenario: Write before receipt
- **WHEN** Markdown already contains an entry marker but its export receipt is missing
- **THEN** retry records the receipt without adding another copy

### Requirement: Inspectable maintenance
Manual deterministic maintenance SHALL persist run outcomes, skip unchanged input without a write, and never call a model. Semantic wiki compilation and recurring schedules SHALL be visibly unavailable in this increment.

#### Scenario: Empty maintenance
- **WHEN** maintenance runs with no pending entries
- **THEN** it records a skipped result with zero model calls and unchanged Markdown
