## ADDED Requirements

### Requirement: A stacked PR follows its parent's branch
A stacked ticket's PR SHALL be rebased onto its own base branch on GitHub, the parent's branch, never onto the repository's base. Whether a rebase is the same change SHALL be judged against the ticket's base as publishing judges it (see ticket-workspaces).

#### Scenario: The parent's branch moves
- **WHEN** a kept stacked PR's parent branch gains a commit on GitHub, and main moves too
- **THEN** the rebase is onto the parent's new tip, is the same change, and is pushed, and the pushed branch does not contain main's new commit
