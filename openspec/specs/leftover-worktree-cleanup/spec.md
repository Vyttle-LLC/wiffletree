# leftover-worktree-cleanup Specification

## Purpose
List leftover ticket worktrees and remove them only after the human approves, never touching dirty or unpushed work.

## Requirements

### Requirement: Dry-run listing of leftover worktrees
The host SHALL list leftover worktrees without changing anything. A leftover is a ticket whose state is `accepted` or `closed`, or whose agents are all archived, whose `worktree` path still exists, and which has no pending worktree removal. The listing SHALL also show, as not removable, every worktree registered in a workspace repository on a `wiffletree/*` branch under the workspaces folder's `tasks` directory that no ticket records. Each entry SHALL show project, repository, ticket title and id, state, worktree path, branch, HEAD commit and one classification.

#### Scenario: Ticket closed before #19
- **WHEN** a ticket was closed by a version before #19 and its worktree still exists
- **THEN** the listing includes it, and no file, worktree, branch or stored row has changed

#### Scenario: Pending removal is not a leftover
- **WHEN** an accepted ticket's worktree waits for its agent's turn to end
- **THEN** the listing does not include it

### Requirement: Dirty and unpushed work is protected
The listing SHALL classify each leftover as exactly one of, in this precedence: `in_turn` (an agent on the ticket is still in its turn), `locked` (the worktree is locked), `dirty` (uncommitted or untracked files, ignored files excluded), `detached` (the worktree is not on the ticket's branch and its HEAD is not contained in that branch), `unpushed` (the ticket branch has commits no remote-tracking branch contains), `untracked_by_wiffletree`, or `clean`. Entries classified `in_turn`, `locked`, `dirty`, `detached` or `untracked_by_wiffletree` SHALL NOT be removable. `clean` and `unpushed` entries SHALL be removable only when individually selected; none SHALL be selected by default.

#### Scenario: Dirty worktree
- **WHEN** a leftover worktree has an untracked file
- **THEN** it is listed as `dirty` and cannot be selected for removal

#### Scenario: Unpushed branch
- **WHEN** a leftover worktree is clean but its branch has a commit on no remote
- **THEN** it is listed as `unpushed`, unselected, with a note that its branch is kept

### Requirement: Removal only after explicit approval
The host SHALL remove leftover worktrees only through a separate removal command carrying the ticket ids the human selected after seeing the listing. For each id it SHALL re-classify the worktree at removal time and skip, with the reason, anything no longer `clean` or `unpushed`. Removal SHALL use non-forced `git worktree remove`, SHALL never delete a branch, ticket, session or message, and SHALL record each outcome in the project's activity.

#### Scenario: Nothing selected
- **WHEN** the human opens the listing and closes it without selecting
- **THEN** nothing is removed

#### Scenario: Worktree changed after listing
- **WHEN** the human selects a `clean` worktree that gained an uncommitted change after the listing
- **THEN** it is skipped as `dirty` and kept

#### Scenario: Approved removal
- **WHEN** the human selects two `clean` worktrees and confirms
- **THEN** both worktree paths are gone, both branches still exist, and the project's activity records two removals
