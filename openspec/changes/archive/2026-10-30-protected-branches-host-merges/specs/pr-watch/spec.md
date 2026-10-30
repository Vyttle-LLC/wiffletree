## MODIFIED Requirements

### Requirement: PR status on ticket cards and repositories
A ticket's card in the Agents panel whose ticket has a PR SHALL show, under its bar, a PR line with the PR number and only the parts that apply: the checks as passed, failed or running; the number of unresolved threads; and one word from the merge state, `conflict`, `draft`, `behind` or `blocked`, the first that applies in that order. A kept ticket's line SHALL end with the PR keeper's part when the keeper has one for the ticket (see pr-keeper). While a merge waits for the ticket (see ticket-merging), the line SHALL end with "merges when they pass" or "merge waits for your approval". A PR into a protected branch SHALL show its base and "yours to merge". A merged or closed PR SHALL instead show `merged` or `closed`; a PR the host merged SHALL show "merged into" its base and "by Wiffletree". Only failed checks and a conflict SHALL use the error color. A ticket without a PR SHALL show no PR line. The ticket's card in the Team panel SHALL describe the PR in full, say when the repository's PRs were last checked, and link to the PR on GitHub; for a PR the host merged it SHALL say so, with the method, the head and why the merge was allowed. Each repository row on the Repositories page SHALL show its number of open PRs on open tickets, and when its PRs were last checked or why the last check failed. A repository that is no longer watched, because each of its open tickets belongs to an archived coordinator, SHALL show no check status.

#### Scenario: Ticket card
- **WHEN** a ticket's PR #142 is open with passing checks, one unresolved thread and its branch behind the base
- **THEN** its Agents panel card shows the line "#142 · checks ✓ · 1 thread · behind"

#### Scenario: Draft that conflicts
- **WHEN** a ticket's draft PR #142 conflicts with its base
- **THEN** its card shows "#142 · conflict" in the error color, and its Team panel card says it is a draft that conflicts with the base

#### Scenario: Merged PR
- **WHEN** a ticket's PR #141 has merged and the ticket is not yet accepted
- **THEN** its card shows "#141 · merged"

#### Scenario: Merged by the host
- **WHEN** the host merged a ticket's PR #180 into `feature/spaces`
- **THEN** its card shows "#180 · merged into feature/spaces · by Wiffletree", and its Team panel card says Wiffletree merged it at the coordinator's request, with the method and head

#### Scenario: Into a protected branch
- **WHEN** a ticket's PR #183 is open into `main` with passing checks
- **THEN** its card shows "#183 · checks ✓ · into main · yours to merge"

#### Scenario: No PR yet
- **WHEN** a ticket has no PR
- **THEN** its card shows no PR line

#### Scenario: Repository row
- **WHEN** a repository has six open tickets, five of them with open PRs, and was checked 40 seconds ago
- **THEN** its row shows "5 open PRs" and that PRs were checked 40 seconds ago

#### Scenario: Check failed
- **WHEN** the last check of a repository failed because `gh` is not logged in
- **THEN** its row shows the failure and when the next check is due, and its tickets keep their PR lines

#### Scenario: Repository no longer watched
- **WHEN** the human archives the coordinator of a repository's only open ticket after its PRs were checked
- **THEN** the repository's row no longer says when its PRs were checked

#### Scenario: The keeper's part
- **WHEN** a kept ticket's PR #143 has running checks and the PR keeper rebased it one minute ago
- **THEN** its card's PR line reads "#143 · checks … · rebased 1 min ago"

#### Scenario: A merge waiting for checks
- **WHEN** the coordinator asked to merge a ticket whose PR #179 has running checks
- **THEN** its card's PR line reads "#179 · checks … · merges when they pass"
