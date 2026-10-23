## MODIFIED Requirements

### Requirement: Read-only PR watching without a model
The host SHALL watch the PRs of open tickets whose coordinator is not archived, using GitHub's API through `gh` and no model session. Each pass SHALL read every such ticket in a repository with one request, or with as few requests as the number of branches requires, and SHALL run off the thread that serves host commands. The watcher SHALL only read from GitHub and Git. It SHALL NOT comment on, label, approve, dismiss reviews of, push to, rebase or merge any PR or branch, and SHALL NOT change any ticket's state. A repository whose base has no remote, or whose remote is not on `github.com`, SHALL NOT be queried and SHALL be shown as not watched, unless an open ticket in it was created with a `base`: pull requests open on `origin`, so the repository SHALL then be read on `origin`.

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
