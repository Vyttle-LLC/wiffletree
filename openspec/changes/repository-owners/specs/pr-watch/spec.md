## MODIFIED Requirements

### Requirement: Polling cadence and backoff
A repository SHALL be checked as soon as the host starts, and within about a minute after it first has an open ticket there. It SHALL also be checked at once after the host pushes a ticket's branch there or opens a ticket's PR there. After a successful pass, it SHALL be checked again after about 60 seconds if any of its open tickets has an open PR, and after about 5 minutes otherwise. After consecutive failures, the next check SHALL wait 5, 10, 20 and then at most 30 minutes, and a success SHALL restore the normal interval. When GitHub reports fewer than 200 remaining API points, the next check SHALL wait at least until GitHub's reported reset time. The host SHALL wake itself for the next due check even when no other event arrives. Checks SHALL continue while a project is paused. Between due checks, the host SHALL read the store for the watched tickets only after something changed, and at most about once a minute, so that ordinary events cost no store read.

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

#### Scenario: After the host opens a PR
- **WHEN** the host opens a ticket's PR in a repository that was checked two minutes ago and is next due in three
- **THEN** the repository is checked at once, and the ticket's row shows the PR without waiting for the interval
