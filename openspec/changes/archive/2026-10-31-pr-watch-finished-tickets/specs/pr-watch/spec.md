## MODIFIED Requirements

### Requirement: Read-only PR watching without a model
The host SHALL watch the PRs of open tickets whose coordinator is not archived, and the PR of a finished (accepted or closed) ticket whose coordinator is not archived while its recorded state is `open`, using GitHub's API through `gh` and no model session. Each pass SHALL read every such ticket in a repository with one request, or with as few requests as the number of branches requires, and SHALL run off the thread that serves host commands. The watcher SHALL only read from GitHub and Git. It SHALL NOT comment on, label, approve, dismiss reviews of, push to, rebase or merge any PR or branch, and SHALL NOT change any ticket's state. A repository whose base has no remote, or whose remote is not on `github.com`, SHALL NOT be queried and SHALL be shown as not watched, unless a watched ticket in it was created with a `base`: pull requests open on `origin`, so the repository SHALL then be read on `origin`.

#### Scenario: One request per repository
- **WHEN** a repository has three open tickets and a pass is due
- **THEN** the host makes one GitHub request for that repository, covering all three ticket branches

#### Scenario: No GitHub remote
- **WHEN** an open ticket's repository has the base `main` with no remote
- **THEN** no request is made for it and its repository row says its PRs are not watched

#### Scenario: Nothing is written
- **WHEN** a ticket's PR changes from passing to failing checks
- **THEN** the host records the new status without commenting, labelling or pushing, and the ticket's state is unchanged

#### Scenario: A ticket on a branch of origin
- **WHEN** a repository's base is `HEAD` and an open ticket in it was created with `base` `release`
- **THEN** the repository's pass reads `origin`

#### Scenario: Only finished tickets to read
- **WHEN** a repository's only watched ticket is an accepted one with a recorded open PR
- **THEN** the repository is still read

#### Scenario: A finished ticket with nothing to learn
- **WHEN** a finished ticket has no PR, or its recorded PR is merged or closed
- **THEN** it is not in any pass

## ADDED Requirements

### Requirement: A finished ticket's PR is only recorded
When a pass reads a finished ticket's PR, the host SHALL save a changed snapshot and log its `pull_request` event as for an open ticket, and a linked issue SHALL follow a new number or state. Once the snapshot reads merged or closed, the ticket SHALL leave the watch, and a later pass SHALL NOT change that record. The change SHALL reach every view of the ticket, including those that read the archive half of the snapshot. A finished ticket's PR SHALL wake nobody: it SHALL send no watcher message, use no automatic wake, be held for no keeper try and give the PR keeper no work, and a message already held back for it SHALL be dropped. It SHALL NOT count as an open PR for a repository's interval, and its base head SHALL NOT be noted for the PR keeper. The host SHALL NOT read the ticket's worktree for it. A repository with only finished tickets to read SHALL show no check status on its row. Records that were stale before this change SHALL heal on the first pass, with no migration.

#### Scenario: Closed after acceptance
- **WHEN** PR #5 of an accepted ticket is recorded as open and a pass reads it closed
- **THEN** the ticket's card shows "#5 · closed", `workspace_context` says closed, nobody receives a message, and the next pass does not read the ticket

#### Scenario: Merged after acceptance
- **WHEN** PR #5 of an accepted ticket, whose worktree is gone, is recorded as open and a pass reads it merged
- **THEN** the record says merged, no merged message is sent and the ticket stays accepted

#### Scenario: Abandoned PR
- **WHEN** a repository's only open PR belongs to an accepted ticket
- **THEN** the repository is checked about every 5 minutes

#### Scenario: A conflict after finishing
- **WHEN** an accepted ticket's open PR starts to conflict with its base
- **THEN** the record says so, and neither the watcher nor the keeper acts on it

#### Scenario: A held message
- **WHEN** a conflict message was held back for the keeper and the ticket is closed with the conflict still there
- **THEN** the next pass drops the message and sends nothing

#### Scenario: A linked issue
- **WHEN** a closed ticket's open PR, linked to CEL-131, is read as merged
- **THEN** CEL-131 gains one `pr` event and derives its state again
