## MODIFIED Requirements

<!-- Delta order for "Safe Markdown export": 1 spaces (this, moved from assistant-spaces-jobs-boards) → 2 brain-log-capture, which removes it. -->

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
