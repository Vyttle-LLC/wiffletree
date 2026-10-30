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
Maintenance SHALL export a project's entries only to its Space's brain (see `spaces`), preserve existing content and compiler markers, use stable entry markers and recover an interrupted export without duplication. A project whose Space has no brain SHALL export nothing. Moving a project to another Space SHALL change where later entries go and SHALL NOT move or re-export earlier ones. Unsafe repository log filenames SHALL be rejected.

#### Scenario: Write before receipt
- **WHEN** Markdown already contains an entry marker but its export receipt is missing
- **THEN** retry records the receipt without adding another copy

#### Scenario: The Space's brain
- **WHEN** a project in the Vyttle Space exports a decision
- **THEN** the entry is written to the Vyttle Space's brain folder, and to no other brain

#### Scenario: Upgraded project
- **WHEN** a project that had a brain attached before Spaces exports after the upgrade
- **THEN** it exports to the same folder through its Space, and entries already exported are not written again

### Requirement: Inspectable maintenance
Manual deterministic maintenance SHALL persist run outcomes, skip unchanged input without a write, and never call a model. Semantic wiki compilation and recurring schedules SHALL be visibly unavailable in this increment.

#### Scenario: Empty maintenance
- **WHEN** maintenance runs with no pending entries
- **THEN** it records a skipped result with zero model calls and unchanged Markdown
