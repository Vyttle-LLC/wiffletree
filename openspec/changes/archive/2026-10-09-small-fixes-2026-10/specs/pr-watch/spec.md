## MODIFIED Requirements

### Requirement: Polling cadence and backoff
A repository SHALL be checked as soon as the host starts, and within about a minute after it first has an open ticket there. After a successful pass, it SHALL be checked again after about 60 seconds if any of its open tickets has an open PR, and after about 5 minutes otherwise. After consecutive failures, the next check SHALL wait 5, 10, 20 and then at most 30 minutes, and a success SHALL restore the normal interval. When GitHub reports fewer than 200 remaining API points, the next check SHALL wait at least until GitHub's reported reset time. The host SHALL wake itself for the next due check even when no other event arrives. Checks SHALL continue while a project is paused. Between due checks, the host SHALL read the store for the watched tickets only after something changed, and at most about once a minute, so that ordinary events cost no store read.

#### Scenario: Active and idle repositories
- **WHEN** one repository has a ticket with an open PR and another has open tickets without PRs
- **THEN** the first is checked about every 60 seconds and the second about every 5 minutes

#### Scenario: Failing checks back off
- **WHEN** `gh` fails for a repository three times in a row
- **THEN** the checks after those failures wait 5, 10 and 20 minutes, and the first success returns the repository to its normal interval

#### Scenario: Rate limit nearly spent
- **WHEN** a pass reports 150 remaining points with a reset in 12 minutes
- **THEN** the repository is not checked again for at least 12 minutes

#### Scenario: Quiet host
- **WHEN** no agent is working and no message arrives for an hour while a ticket has an open PR
- **THEN** the repository is still checked about every 60 seconds

#### Scenario: Paused project
- **WHEN** the human pauses a project whose open ticket has an open PR
- **THEN** that ticket's repository is still checked

#### Scenario: Busy host
- **WHEN** agents produce many changes within a minute and no check is due
- **THEN** the watcher reads the store for its tickets at most once in that minute

### Requirement: PR status on tickets and repositories
A sidebar ticket row whose ticket has a PR SHALL show a second line with the PR number and only the parts that apply: the checks as passed, failed or running; the number of unresolved threads; and one word from the merge state, `conflict`, `draft`, `behind` or `blocked`, the first that applies in that order. A merged or closed PR SHALL instead show `merged` or `closed`. Only failed checks and a conflict SHALL use the error color. A ticket without a PR SHALL keep a single-line row. The row's tooltip and the ticket's card in the Team panel SHALL describe the PR in full, say when the repository's PRs were last checked, and link to the PR on GitHub. Each repository row on the Repositories page SHALL show its number of open PRs on open tickets, and when its PRs were last checked or why the last check failed. A repository that is no longer watched, because each of its open tickets belongs to an archived coordinator, SHALL show no check status.

#### Scenario: Ticket row
- **WHEN** a ticket's PR #142 is open with passing checks, one unresolved thread and its branch behind the base
- **THEN** its row shows a second line "#142 · checks ✓ · 1 thread · behind"

#### Scenario: Draft that conflicts
- **WHEN** a ticket's draft PR #142 conflicts with its base
- **THEN** its row shows "#142 · conflict" in the error color, and its tooltip says it is a draft that conflicts with the base

#### Scenario: Merged row
- **WHEN** a ticket's PR #141 has merged and the ticket is not yet accepted
- **THEN** its row shows "#141 · merged"

#### Scenario: No PR yet
- **WHEN** a ticket has no PR
- **THEN** its row is unchanged

#### Scenario: Repository row
- **WHEN** a repository has six open tickets, five of them with open PRs, and was checked 40 seconds ago
- **THEN** its row shows "5 open PRs" and that PRs were checked 40 seconds ago

#### Scenario: Check failed
- **WHEN** the last check of a repository failed because `gh` is not logged in
- **THEN** its row shows the failure and when the next check is due, and its tickets keep their PR lines

#### Scenario: Repository no longer watched
- **WHEN** the human archives the coordinator of a repository's only open ticket after its PRs were checked
- **THEN** the repository's row no longer says when its PRs were checked
