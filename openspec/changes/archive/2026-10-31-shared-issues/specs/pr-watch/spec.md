## MODIFIED Requirements

### Requirement: PR snapshot on the ticket
Each ticket SHALL record its PR's number, URL, state (`open`, `closed` or `merged`), draft flag, base branch, head SHA, merge state, check rollup (`none`, `pending`, `success` or `failure`), number of unresolved review threads, newest comment id of each kind (issue comments, reviews and replies inside review threads) and merge commit. The id of a kind SHALL change whenever a comment of that kind is added, whatever its numeric value relative to the other kinds. The coordinator SHALL see this record in `workspace_context`. The host SHALL store the record and log a `pull_request` activity event only when it changes. A pass in which GitHub reports the merge state as unknown SHALL keep the previously recorded merge state. A failed pass SHALL keep every recorded snapshot. Tickets stored before this change SHALL load with no PR. When a ticket's recorded PR number or state changes and the ticket is linked to an issue, itself or through its project, the host SHALL append a `pr` event with the PR's URL and state to that issue and derive the issue's state again, as shared-issues says. The watcher itself SHALL still write nothing to GitHub.

#### Scenario: Change recorded once
- **WHEN** a pass records a change to a ticket's PR and the next pass returns the same state
- **THEN** the next pass stores nothing and logs no event

#### Scenario: Unknown merge state
- **WHEN** a PR recorded as `behind` reads as unknown on the next pass with nothing else changed
- **THEN** the ticket still records `behind` and no event is logged

#### Scenario: Coordinator reads the PR
- **WHEN** the coordinator calls `workspace_context` for a ticket with PR #142
- **THEN** the ticket's entry includes PR #142's number, state, head, merge state, checks and unresolved threads

#### Scenario: Failed pass
- **WHEN** `gh` is not logged in during a pass
- **THEN** every ticket keeps its last recorded PR

#### Scenario: A linked ticket's PR opens
- **WHEN** a pass first records PR #412 as open for a ticket whose project is linked to CEL-131
- **THEN** CEL-131 gains a `pr` event for #412, moves to In review, and nothing is written to GitHub

#### Scenario: An unlinked ticket
- **WHEN** a pass records a merge for a ticket whose project is linked to no issue
- **THEN** no tracker file changes
