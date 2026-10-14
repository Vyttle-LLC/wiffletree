## MODIFIED Requirements

### Requirement: Removal only after explicit approval
The host SHALL remove leftover worktrees only through a separate removal command carrying the ticket ids the human selected after seeing the listing. For each id it SHALL re-classify the worktree at removal time and skip, with the reason, anything no longer `clean` or `unpushed`. Removal SHALL be the whole removal of a ticket's worktree, never forced past a lock or unsaved work, SHALL never delete a branch, ticket, session or message, and SHALL record each outcome in the project's activity.

#### Scenario: Nothing selected
- **WHEN** the human opens the listing and closes it without selecting
- **THEN** nothing is removed

#### Scenario: Worktree changed after listing
- **WHEN** the human selects a `clean` worktree that gained an uncommitted change after the listing
- **THEN** it is skipped as `dirty` and kept

#### Scenario: Approved removal
- **WHEN** the human selects two `clean` worktrees and confirms
- **THEN** both worktree paths are gone, both branches still exist, and the project's activity records two removals
