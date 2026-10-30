## ADDED Requirements

### Requirement: Every cycle starts at once
`verify_ticket` and the PR keeper's review SHALL start every verifier of the cycle's first round at once, whatever other cycles are running; no cycle SHALL wait for a verification slot, and the result SHALL NOT report the cycle as queued. The build gate, not the cycle, bounds how many builds run. A cycle an earlier host stored as queued for a verification slot SHALL start at the next scheduler pass on a live project, pinned to the worktree's HEAD then, and SHALL end blocked, telling the coordinator, if the worktree has uncommitted or untracked files or a verifier it needs was archived.

#### Scenario: A third cycle
- **WHEN** two cycles are running and the coordinator verifies a third ticket
- **THEN** the third cycle's verifiers get their round message at once

#### Scenario: Upgrade with a queued cycle
- **WHEN** the store holds a cycle queued for a verification slot and the worktree has a newer commit
- **THEN** after the host starts, the cycle starts at that commit

#### Scenario: Upgrade with a dirty worktree
- **WHEN** the store holds a queued cycle and its worktree has uncommitted files
- **THEN** the cycle ends blocked and the coordinator is told why

## MODIFIED Requirements

### Requirement: A review goes ahead without a skipped verifier
When a cycle is created, a verifier of its level whose row resolves to skip (see model-routing) SHALL start no session and SHALL be recorded on the cycle as skipped, with its role, focus and a one-line reason such as "Claude's weekly limit was at 96%; its row says skip". A skipped verifier SHALL take no part in any round of that cycle, SHALL never block it or count as pending. When every verifier of the level would skip, no cycle SHALL start: `verify_ticket` SHALL refuse with each verifier's reason, and a keeper review SHALL NOT start. The cycle's outcome message SHALL name each skipped verifier and why. A cycle that passed with skipped verifiers SHALL be treated by `accept_ticket` and `publish_ticket` as any passed cycle. The Tickets inspector SHALL show a Skipped pill with the reason for each skipped verifier, and `workspace_context` SHALL list them on the cycle.

#### Scenario: One verifier skipped
- **WHEN** the level "Full" runs Tests, Claude and Codex, Claude's row says skip and Claude's weekly window is at 96%
- **THEN** Tests and Codex start, Claude starts no session and is recorded as skipped with "Claude's weekly limit was at 96%; its row says skip"
- **AND** when Tests and Codex pass, the cycle passes, the coordinator's message names Claude as skipped and why, and the ticket can be accepted and published as after a full pass

#### Scenario: Every verifier would skip
- **WHEN** every verifier of the level says skip and each one's first choice is spent
- **THEN** `verify_ticket` is refused with each verifier's reason, no session starts and the ticket's state is unchanged
