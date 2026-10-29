## MODIFIED Requirements

### Requirement: Where the PR keeper acts
The PR keeper SHALL be host code that acts after the PR watcher's passes, with no model session to watch or decide. A ticket's PR SHALL be kept only when all of these hold: the ticket is open, its coordinator is not archived, its project is not paused, its repository publishes without asking (see repository-owners), and its PR, as the watcher last read it, is open. For any other PR the keeper SHALL do nothing and SHALL raise no Inbox item, and the PR SHALL be watched as before. A PR that is not the PR of one of this workspace's tickets SHALL never be kept.

The keeper SHALL change a kept ticket's branch only when these also hold: the ticket's latest verification cycle passed, or ended blocked and the ticket records the coordinator's waiver of it (see ticket-publishing), and none is running; the worktree's HEAD is the commit that cycle verified or a patch-equivalent one; the PR's head is the worktree's HEAD; the ticket records no head that is not reviewed (see "Commits Wiffletree did not make are adopted"); and no publish request for the ticket is running or waiting. It SHALL do at most one thing to a ticket's branch at a time. Adopting commits Wiffletree did not make has its own conditions (see below).

#### Scenario: Owner not allowed
- **WHEN** a ticket in `celadon-theme/website` has an open PR that is behind its base, `celadon-theme` is not allowed, and every trigger is on
- **THEN** nothing is rebased, tried or sorted for it, no Inbox item is raised, and its PR line reads as it does without the keeper

#### Scenario: The owner is unticked later
- **WHEN** the human unticks an owner while one of its repositories' PRs is due for a rebase
- **THEN** no rebase is pushed for it

#### Scenario: A review that ended blocked
- **WHEN** a kept ticket's latest cycle ended blocked, the ticket records no waiver, and its PR is behind its base
- **THEN** its branch is not changed

#### Scenario: A waived blocked review
- **WHEN** a kept ticket's latest cycle ended blocked, its coordinator published it with a waiver, and its PR is behind its base
- **THEN** the keeper may rebase the branch and push the rebase when its patch is the reviewed commit's

#### Scenario: An agent has moved the worktree on
- **WHEN** a kept ticket's worktree has a commit the PR does not have
- **THEN** the keeper does not change the branch, and waits

#### Scenario: A paused project
- **WHEN** the human pauses a project whose kept ticket's PR is behind its base
- **THEN** its PR is still watched and nothing is rebased, tried or sorted

#### Scenario: A PR that is no ticket's
- **WHEN** an allowed owner's repository has an open PR whose branch is no ticket's branch
- **THEN** the keeper does nothing for it
