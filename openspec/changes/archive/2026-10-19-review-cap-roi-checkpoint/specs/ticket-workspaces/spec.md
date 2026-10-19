## MODIFIED Requirements

### Requirement: Finishing a ticket
`accept_ticket` SHALL take a ticket id and an optional one-line waiver reason. It SHALL depend on the ticket's latest verification cycle, not on the ticket's state or the kind of the last report. Every acceptance SHALL require that no cycle is running, that the latest cycle's verified commit equals the worktree's HEAD or is patch-equivalent to it, or that the waiver covers the commits past it as below, and that the ticket's ledger has no `untriaged` entries. A HEAD is patch-equivalent when the diff from its merge-base with the repository's base has the same stable patch id as the verified commit's diff from its own merge-base. When the patch id cannot be computed, only an equal commit SHALL qualify, and the refusal SHALL say why. No acceptance, with or without a waiver, SHALL be given while the ticket records a head that is not reviewed, as after the PR keeper adopted commits Wiffletree did not make (see pr-keeper), whatever the HEAD's patch id; the refusal SHALL say that the branch has commits no review has checked. Without a waiver, the latest cycle SHALL have passed. With a waiver, the latest cycle SHALL have ended `blocked` with no verifier whose latest result is `blocked` or still pending; a waiver after a passed cycle SHALL be refused, unless it covers commits no review checked. Once the ticket has started as many full reviews as the cycle cap allows, a waiver MAY cover the commits from the verified commit to HEAD when it names every one of them by a short SHA of at least 7 characters; the refusal SHALL list the short SHAs of any it does not name. Below the cap, a waiver SHALL NOT cover such commits, and they need a review. A waived acceptance SHALL record the commits it covered with the reason. A waived acceptance SHALL record the reason on the ticket and record the `ticket_accepted` event with the reason and the ids of the entries still `open`; the cycle's outcome SHALL stay `blocked` and those entries SHALL stay `open`. Accepting or closing a ticket SHALL archive the ticket's agents, keep their conversations and the branch, and remove the worktree, refusing while the worktree is locked or holds uncommitted or untracked files. `close_ticket` SHALL also refuse while any of the ticket's agents has status Working, naming that agent; once an agent has reported, a turn it is still finishing only defers removal of the worktree, for accept and close alike. Neither SHALL merge, push or publish.

#### Scenario: Commits after verification
- **WHEN** the project coordinator accepts a passed ticket whose worktree HEAD adds a commit that changes the diff after the verified commit
- **THEN** the host refuses, naming the verified and current commits

#### Scenario: Unreviewed commits at the cap
- **WHEN** a ticket has used every full review the cycle cap allows, its latest cycle passed, two commits follow the verified one, and the coordinator accepts with a waiver naming both by short SHA
- **THEN** the ticket is `accepted` and records the waiver with both commits

#### Scenario: A commit the waiver does not name
- **WHEN** at the cap the waiver names only the first of those commits
- **THEN** `accept_ticket` refuses, naming the second commit's short SHA, and the ticket stays open

#### Scenario: Below the cap
- **WHEN** the ticket may still start a full review and the coordinator accepts with a waiver naming every commit after the verified one
- **THEN** `accept_ticket` refuses, and the commits need a review

#### Scenario: Squashed after verification
- **WHEN** the implementer squashes the verified commits into one commit with the same diff and the coordinator accepts the ticket
- **THEN** the ticket is `accepted` without a new cycle

#### Scenario: Report after a passed cycle
- **WHEN** a cycle passed at HEAD, the implementer then reports `completed` after opening a PR, and the coordinator accepts the ticket after the PR merges
- **THEN** the ticket is `accepted`

#### Scenario: Untriaged findings block acceptance
- **WHEN** the latest cycle passed and the ledger still has an `untriaged` entry
- **THEN** `accept_ticket` refuses, naming the entry

#### Scenario: Accept with a waiver
- **WHEN** cycle 2 ended `blocked` at the round cap with F3 `open`, no verifier reported `blocked`, the ledger has no `untriaged` entry, and the coordinator accepts with the waiver "F3 needs a base ref the host always fetches"
- **THEN** the ticket is `accepted` with that waiver, cycle 2's outcome is still `blocked`, F3 is still `open`, and the `ticket_accepted` event names the waiver and F3

#### Scenario: Waiver refused
- **WHEN** the latest cycle ended `blocked` because a verifier reported `blocked`, or the latest cycle passed, and the coordinator accepts with a waiver
- **THEN** the host refuses and the ticket is unchanged

#### Scenario: Accepting archives the workspace
- **WHEN** the project coordinator accepts a passed ticket with a clean worktree and no agent in its turn
- **THEN** the ticket is `accepted`, its agents are archived with their conversations readable, its branch exists and its worktree path is gone

#### Scenario: Adopted commits with the same patch id
- **WHEN** the PR keeper adopted a commit that only re-indents a line, so the worktree's HEAD has the verified commit's stable patch id, and the coordinator accepts the ticket before any later review
- **THEN** the host refuses, saying the branch has commits no review has checked
