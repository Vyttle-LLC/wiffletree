## ADDED Requirements

### Requirement: The host may publish for the PR keeper
The host SHALL be able to start a publish of a ticket itself, for the PR keeper, without a `publish_ticket` call. Such a publish SHALL only push the ticket's branch to its open pull request; it SHALL NOT open a pull request and SHALL take no title or description. It SHALL be subject to every check, at every moment, that a publish requested with `publish_ticket` is: the ticket is open, reviewed and has no cycle running; the worktree is on the ticket's branch at the reviewed commit or a patch-equivalent one; `origin` is one checked place on GitHub that the base tracks; the frozen request is compared again before anything is sent; the owner is confirmed with GitHub; exactly one ref is sent to the frozen URL; and GitHub's branch is replaced only when it is the commit the host last pushed or was once this branch's tip. No rule SHALL be relaxed for the keeper.

A keeper publish SHALL be started only for a ticket whose publish would run without asking. It SHALL never raise an Inbox item: when a step would need the human's approval, as when the owner was unticked or a later review ended blocked, the request SHALL end with nothing sent. Its result SHALL go to the keeper's record of the ticket and SHALL NOT be sent to any agent as a publish result. While its step runs, `publish_ticket` for the ticket SHALL be refused as for any running step. A keeper publish that was running when the host stopped SHALL be ended at the next start with nothing more sent, and the next pass SHALL decide anew.

#### Scenario: A rebased branch is pushed for the keeper
- **WHEN** the keeper has moved a patch-equivalent rebase onto a ticket's branch in an allowed owner's repository whose latest cycle passed, and GitHub's branch is at the commit the host last pushed
- **THEN** the host replaces GitHub's branch with the rebased commit under a lease on that commit, raises no Inbox item, and sends no agent a publish result

#### Scenario: GitHub's branch has someone else's commit
- **WHEN** GitHub's branch is at a commit the host did not push and this branch never had
- **THEN** the host refuses, pushes nothing, and records the refusal for the keeper

#### Scenario: A commit the review did not check
- **WHEN** a keeper publish is started while the worktree's HEAD has changes the ticket's latest cycle did not check
- **THEN** it is refused and nothing is sent

#### Scenario: The owner was unticked
- **WHEN** the human unticks the repository's owner after the keeper's rebase and before its push
- **THEN** nothing is pushed and no Inbox item is raised

#### Scenario: A blocked review
- **WHEN** the ticket's latest cycle ended blocked
- **THEN** no keeper publish starts

#### Scenario: Transferred repository
- **WHEN** GitHub reports the repository under another owner when the keeper's push is about to run
- **THEN** nothing is pushed, as for any publish

#### Scenario: Only the ticket's branch
- **WHEN** Git is configured to push tags and all branches
- **THEN** the keeper's push creates or updates exactly one ref, the ticket's branch

#### Scenario: An agent publishes meanwhile
- **WHEN** the ticket's implementer calls `publish_ticket` while the keeper's push is running
- **THEN** the call is refused, saying a publish step for this ticket is running

#### Scenario: Interrupted by a restart
- **WHEN** the host stops while a keeper push is running and starts again
- **THEN** the request is ended, no agent is messaged, and a later pass pushes again only if the checks still pass

## MODIFIED Requirements

### Requirement: Checks before anything leaves the machine
Before pushing or asking the human, the host SHALL refuse a publish unless all of these hold: the ticket is open; the ticket has a verification cycle and none is running; the worktree's HEAD is the commit that cycle's last round checked, or a patch-equivalent one such as a squash or a clean rebase; the worktree is on the ticket's branch; and the repository's `origin` is on `github.com`, has exactly one push URL, has a fetch URL naming the same repository, still names the repository the host recorded for it, and is the remote its base tracks. The host SHALL also refuse while the ticket records a head that is not reviewed, as after the PR keeper adopted commits Wiffletree did not make (see pr-keeper), whatever the HEAD's patch id; the refusal SHALL say that the branch has commits no review has checked. A refusal SHALL say why and SHALL send nothing. No answer from the human SHALL override these checks. The host SHALL make the same checks again, for the same commit, when the human approves a step and immediately before it sends anything. A request SHALL record the verification cycle it was decided on and how that cycle ended. If the ticket's latest cycle is no longer that one at either of those moments, the host SHALL send nothing, SHALL end the request and SHALL tell the caller to publish again, so that the later cycle decides whether the human is asked.

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
- **THEN** nothing is pushed, the caller is told the review changed, and publishing again waits for the human's approval

#### Scenario: HEAD moves while a card waits
- **WHEN** the human approves a push after the worktree's HEAD has moved to different changes
- **THEN** nothing is pushed and the caller is told

#### Scenario: Adopted commits with the same patch id
- **WHEN** the PR keeper adopted a commit that only re-indents a line, so the worktree's HEAD has the verified commit's stable patch id, and `publish_ticket` is called before any later review
- **THEN** it is refused, saying the branch has commits no review has checked, and nothing is sent

#### Scenario: After the adopted head was reviewed
- **WHEN** a review started after the adoption has passed at the adopted head and `publish_ticket` is called
- **THEN** the publish goes ahead
