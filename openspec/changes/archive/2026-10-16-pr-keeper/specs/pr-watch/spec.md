## ADDED Requirements

### Requirement: The base branch's head is read with each pass
Each pass SHALL also read, for every PR it finds, the head commit of the PR's base branch as GitHub reports it, in the same request. The host SHALL NOT store it on the ticket's PR snapshot and SHALL NOT log an activity event when it changes; it is kept in memory for the PR keeper (see pr-keeper).

#### Scenario: A moving base logs nothing
- **WHEN** a repository's base branch gains five commits in an hour and a ticket's PR is otherwise unchanged
- **THEN** no `pull_request` event is logged for that ticket and its stored snapshot is unchanged

#### Scenario: Still one request
- **WHEN** a repository has three open tickets and a pass is due
- **THEN** the host makes one GitHub request, and it carries each PR's base head

## MODIFIED Requirements

### Requirement: PR status on tickets and repositories
A sidebar ticket row whose ticket has a PR SHALL show a second line with the PR number and only the parts that apply: the checks as passed, failed or running; the number of unresolved threads; and one word from the merge state, `conflict`, `draft`, `behind` or `blocked`, the first that applies in that order. A kept ticket's line SHALL end with the PR keeper's part when the keeper has one for the ticket (see pr-keeper). A merged or closed PR SHALL instead show `merged` or `closed`. Only failed checks and a conflict SHALL use the error color. A ticket without a PR SHALL keep a single-line row. The row's tooltip and the ticket's card in the Team panel SHALL describe the PR in full, say when the repository's PRs were last checked, and link to the PR on GitHub. Each repository row on the Repositories page SHALL show its number of open PRs on open tickets, and when its PRs were last checked or why the last check failed. A repository that is no longer watched, because each of its open tickets belongs to an archived coordinator, SHALL show no check status.

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

#### Scenario: The keeper's part
- **WHEN** a kept ticket's PR #143 has running checks and the PR keeper rebased it one minute ago
- **THEN** its row's second line reads "#143 · checks … · rebased 1 min ago"

### Requirement: Failed checks and conflicts wake the coordinator within a budget
When an open PR's recorded checks become `failure`, the host SHALL send the ticket's coordinator one turn-starting message for that head SHA. When an open PR's recorded merge state becomes a conflict with its base, the host SHALL likewise send one message for that head SHA. These messages SHALL name the ticket, the PR number, the head and, for a conflict, the base. They SHALL say that a fix needs a new verification cycle before it is pushed. Together, they SHALL be limited to 3 per PR. The next one due after that SHALL be replaced by a single message saying the PR has used its automatic wakes, after which the PR SHALL wake nobody until it merges or closes. These messages SHALL go only to the coordinator, never to the implementer. The host SHALL hold such a message back for a head while the PR keeper will try that problem at that head, and SHALL NOT send it once the keeper has started a try for it (see pr-keeper): the keeper's own message, sent only when its try is thrown away, takes its place. The host SHALL record each message it holds back, and SHALL send it, in the same words, on the first later pass at which the PR still has that problem at that head, no try for it has started and the keeper can no longer try it. A held message SHALL be dropped when the PR's head changes or the problem is gone. Keeper messages SHALL NOT count against the limit of 3. PR comments SHALL wake nobody.

#### Scenario: Checks fail
- **WHEN** PR #143's checks fail at head `9e03c1b`
- **THEN** the coordinator receives one message naming PR #143 and head `9e03c1b`, and the implementer receives nothing

#### Scenario: Same head, no repeat
- **WHEN** checks at the same head are re-run and fail again
- **THEN** no further message is sent

#### Scenario: Conflict
- **WHEN** PR #139's base moves on and the PR now conflicts with it at head `3daf610`
- **THEN** the coordinator receives one message naming PR #139, its base and head `3daf610`

#### Scenario: Budget used
- **WHEN** a PR has already woken the coordinator 3 times and its checks fail at a new head
- **THEN** the coordinator receives one message saying the PR has used its automatic wakes, and later failures or conflicts on that PR send nothing

#### Scenario: New comment
- **WHEN** a reviewer comments on a ticket's PR, or replies inside an existing unresolved review thread
- **THEN** the recorded comment id of that kind changes and no agent is woken

#### Scenario: The keeper will try the conflict
- **WHEN** PR #139 conflicts with its base at head `3daf610`, the rebase and conflicts triggers are on, its repository is kept and has a checks command, its branch may be changed and no try is recorded for that head
- **THEN** the coordinator receives no watcher message for that conflict

#### Scenario: The keeper cannot try
- **WHEN** PR #139 conflicts at head `3daf610` in a kept repository with no checks command
- **THEN** the coordinator receives the watcher's message, as without the keeper

#### Scenario: The keeper can no longer try
- **WHEN** a conflict message was held back, and before a try starts the ticket's implementer commits in the worktree
- **THEN** the next pass sends the held message, naming PR #139, its base and head `3daf610`

#### Scenario: The try was thrown away
- **WHEN** the keeper's try at that conflict was thrown away and the coordinator was told by the keeper
- **THEN** no watcher message is sent for that head

#### Scenario: The problem went away
- **WHEN** a conflict message was held back and the keeper's fix is pushed, so the PR has a new head
- **THEN** the held message is dropped and never sent
