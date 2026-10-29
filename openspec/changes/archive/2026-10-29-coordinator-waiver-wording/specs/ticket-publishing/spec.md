## MODIFIED Requirements

### Requirement: Checks before anything leaves the machine
Before pushing or asking the human, the host SHALL refuse a publish unless all of these hold: the ticket is open; the ticket has a verification cycle and none is running; the worktree's HEAD is the commit that cycle's last round checked, or a patch-equivalent one such as a squash or a clean rebase, or, once the ticket has started as many full reviews as the cycle cap allows, a later one whose commits since the checked one the call's `waived` reason names, each by a short SHA of at least 7 characters, or one patch-equivalent to the last commit a waiver recorded on the ticket covered, as after the PR keeper's rebase; the worktree is on the ticket's branch; and the repository's `origin` is on `github.com`, has exactly one push URL, has a fetch URL naming the same repository, still names the repository the host recorded for it, and is the remote its base tracks. The host SHALL also refuse while the ticket records a head that is not reviewed, as after the PR keeper adopted commits Wiffletree did not make (see pr-keeper), whatever the HEAD's patch id; the refusal SHALL say that the branch has commits no review has checked. A refusal SHALL say why and SHALL send nothing. No answer from the human SHALL override these checks. A waiver that leaves a commit out SHALL be refused with that commit's short SHA, and after a passed cycle a waiver when no commit needs one SHALL be refused; after a blocked cycle the waiver covers its open findings (see "The coordinator's waiver publishes without asking"). The host SHALL make the same checks again, with the same waived commits, for the same commit, when the human approves a step and immediately before it sends anything. A request SHALL record the verification cycle it was decided on and how that cycle ended. If the ticket's latest cycle is no longer that one at either of those moments, the host SHALL send nothing, SHALL end the request and SHALL tell the caller to publish again, so that the later cycle decides whether the human is asked.

#### Scenario: Never reviewed
- **WHEN** `publish_ticket` is called for a ticket that has had no verification cycle
- **THEN** it is refused, saying the ticket has not been reviewed

#### Scenario: Review still running
- **WHEN** `publish_ticket` is called while a verification cycle is running
- **THEN** it is refused, saying to publish once the cycle ends

#### Scenario: HEAD moved past the reviewed commit
- **WHEN** the worktree's HEAD has changes the last cycle did not check
- **THEN** it is refused, naming the reviewed and current commits

#### Scenario: Squash after review
- **WHEN** the reviewed commits were squashed into one commit with the same changes
- **THEN** the publish goes ahead

#### Scenario: Wrong branch
- **WHEN** the worktree is on `main` rather than the ticket's branch
- **THEN** it is refused and nothing is pushed

#### Scenario: Not on GitHub
- **WHEN** the ticket's repository has no GitHub `origin`
- **THEN** it is refused, saying there is nowhere to push

#### Scenario: A second push URL
- **WHEN** the repository's `origin` has two push URLs, the first under an allowed owner and the second under another owner
- **THEN** `publish_ticket` is refused and nothing is pushed to either

#### Scenario: A later review ends blocked before a queued push sends
- **WHEN** a push was queued without asking after cycle 1 passed, and before it sends a second cycle of the same commit ends blocked
- **THEN** nothing is pushed, the caller is told the review changed, and publishing again waits for the human's approval unless the coordinator waives the new cycle

#### Scenario: HEAD moves while a card waits
- **WHEN** the human approves a push after the worktree's HEAD has moved to different changes
- **THEN** nothing is pushed and the caller is told

#### Scenario: Adopted commits with the same patch id
- **WHEN** the PR keeper adopted a commit that only re-indents a line, so the worktree's HEAD has the verified commit's stable patch id, and `publish_ticket` is called before any later review
- **THEN** it is refused, saying the branch has commits no review has checked, and nothing is sent

#### Scenario: After the adopted head was reviewed
- **WHEN** a review started after the adoption has passed at the adopted head and `publish_ticket` is called
- **THEN** the publish goes ahead

#### Scenario: A rebase of commits waived at the cap
- **WHEN** at the cap the coordinator published commits past the verified one under a waiver naming them, and the PR keeper later rebased the branch without changing its patch
- **THEN** the rebased head counts as the waived one, and its push goes ahead without asking in an allowed owner's repository

### Requirement: The approval names the review level
A publish approval's Review row SHALL name the level of the ticket's latest verification cycle and SHALL say when that cycle was a re-check. When the latest cycle is a re-check, the approval SHALL also show a "Last full review" row naming the most recent passed full review of the ticket: its cycle, its level, the commit it verified and its verifiers. A cycle recorded without a level SHALL be shown without one and SHALL count as a full review. The level SHALL NOT change whether the human is asked: a passed re-check SHALL publish as any passed review does, and a review that ended blocked SHALL need the human's approval at every level unless the coordinator waived it.

#### Scenario: After a full review
- **WHEN** a push waits for approval and the ticket's latest cycle is cycle 1, a passed "Full" review by Tests, Correctness and Regressions at the commit being pushed
- **THEN** the item's Review row reads "Cycle 1 · Full passed at this commit · Tests, Correctness, Regressions", and it has no "Last full review" row

#### Scenario: After a re-check
- **WHEN** a push waits for approval, cycle 1 was a passed "Full" review at `9e03c1b`, and the latest cycle is cycle 2, a passed "Light" re-check by Correctness at the commit being pushed
- **THEN** the Review row reads "Cycle 2 · Light re-check passed at this commit · Correctness"
- **AND** a "Last full review" row reads "Cycle 1 · Full passed at 9e03c1b · Tests, Correctness, Regressions"

#### Scenario: A re-check that ended blocked
- **WHEN** `publish_ticket` is called without `waived` in an allowed owner's repository for a ticket whose latest cycle, a "Light" re-check, ended blocked
- **THEN** nothing is sent, and the approval says the review ended blocked, names it as a Light re-check, lists the open findings and shows the "Last full review" row

#### Scenario: A passed re-check in an allowed owner's repository
- **WHEN** a ticket in an allowed owner's repository has an open pull request, its latest cycle is a passed re-check at HEAD, and `publish_ticket` is called
- **THEN** the host pushes the commit, and no Inbox item is raised

#### Scenario: A cycle from before levels
- **WHEN** a push waits for approval and the ticket's latest cycle passed and was recorded without a level
- **THEN** the Review row reads as before, "Cycle 1 passed at this commit" with its verifiers, and there is no "Last full review" row
