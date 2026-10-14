## ADDED Requirements

### Requirement: Removing a ticket's worktree is whole whatever its size
Removing a ticket's worktree, on accept, close, archive, a pending removal or an approved leftover removal, SHALL either remove it completely or leave it whole at its path with all its files, whatever the size of its folder, including ignored build output. The host SHALL rename the folder aside on the same filesystem, have Git forget the worktree within the usual Git time limit, and then delete the renamed folder without a time limit and without holding up other host work. If Git fails and still keeps the worktree's registration, the host SHALL rename the folder back and the removal SHALL fail as before; if Git had already forgotten it, the removal SHALL complete. At startup the host SHALL delete any folder a stop left renamed aside. The ticket's branch SHALL never be deleted.

#### Scenario: A large worktree is removed whole
- **WHEN** a ticket whose worktree holds thousands of ignored build files is closed
- **THEN** the ticket is closed, its worktree path is gone, Git no longer lists the worktree, the renamed folder is deleted and the branch remains

#### Scenario: Git fails to forget the worktree
- **WHEN** Git fails while forgetting the worktree of a ticket being closed
- **THEN** the close fails, the ticket stays open, and the worktree is registered at its path with all its files and no unsaved changes

#### Scenario: Git fails after forgetting the worktree
- **WHEN** Git forgets the worktree of a ticket being closed and then fails, for example by reaching its time limit
- **THEN** the removal completes: the ticket is closed, its worktree path is gone and the renamed folder is deleted

#### Scenario: A stop during deletion
- **WHEN** the host starts and a ticket's renamed-aside folder is still present
- **THEN** that folder is deleted and the ticket's worktree is untouched
