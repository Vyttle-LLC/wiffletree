## ADDED Requirements

### Requirement: A waiver of unreviewed commits always needs the human's approval
A publish whose `waived` reason covers commits no review checked SHALL NOT push or open a pull request without the human's approval in the Inbox, in every repository, including an allowed owner's after a passed review. The request SHALL record the reason and the commits it covers. The item SHALL list the short SHA of each unreviewed commit and the waiver's reason. In an allowed owner's repository there SHALL be one such approval for the commit, naming everything that would be sent, and approving it SHALL run the commit's steps without a further item; elsewhere each step's approval SHALL show them. The rule that a blocked review always needs the human's approval SHALL be unchanged.

#### Scenario: Allowed owner, passed review, waived commits
- **WHEN** at the cap `publish_ticket` is called with `waived` naming the one commit after the reviewed one, in an allowed owner's repository whose latest cycle passed
- **THEN** nothing is sent, and the Inbox shows one approval listing that commit's short SHA and the waiver's reason

## MODIFIED Requirements

### Requirement: The host publishes a ticket on request
The host SHALL offer agents one tool, `publish_ticket`, taking a ticket id, an optional title and description, and an optional one-line `waived` reason. The host itself SHALL run the push and open the pull request; the tool SHALL NOT take a branch, a remote or a base. It SHALL push the ticket's own branch to the repository's `origin`. When the ticket has no open pull request, it SHALL then open one from that branch into the repository's configured base branch, and the title and description SHALL be required. When the ticket has an open pull request, it SHALL only push, and SHALL refuse a title or description.

#### Scenario: First publish
- **WHEN** `publish_ticket` is called with a title and description for a reviewed ticket with no pull request
- **THEN** the host pushes the ticket's branch and opens a pull request into the repository's base branch with that title and description

#### Scenario: Update
- **WHEN** `publish_ticket` is called for a ticket whose pull request is open
- **THEN** the host pushes the ticket's branch and opens nothing

#### Scenario: Missing title
- **WHEN** `publish_ticket` is called without a title for a ticket with no open pull request
- **THEN** it is refused and nothing is sent

### Requirement: Checks before anything leaves the machine
Before pushing or asking the human, the host SHALL refuse a publish unless all of these hold: the ticket is open; the ticket has a verification cycle and none is running; the worktree's HEAD is the commit that cycle's last round checked, or a patch-equivalent one such as a squash or a clean rebase, or, once the ticket has started as many full reviews as the cycle cap allows, a later one whose commits since the checked one the call's `waived` reason names, each by a short SHA of at least 7 characters; the worktree is on the ticket's branch; and the repository's `origin` is on `github.com`, has exactly one push URL, has a fetch URL naming the same repository, still names the repository the host recorded for it, and is the remote its base tracks. The host SHALL also refuse while the ticket records a head that is not reviewed, as after the PR keeper adopted commits Wiffletree did not make (see pr-keeper), whatever the HEAD's patch id; the refusal SHALL say that the branch has commits no review has checked. A refusal SHALL say why and SHALL send nothing. No answer from the human SHALL override these checks. A waiver that leaves a commit out SHALL be refused with that commit's short SHA, and a waiver when no commit needs one SHALL be refused. The host SHALL make the same checks again, with the same waived commits, for the same commit, when the human approves a step and immediately before it sends anything. A request SHALL record the verification cycle it was decided on and how that cycle ended. If the ticket's latest cycle is no longer that one at either of those moments, the host SHALL send nothing, SHALL end the request and SHALL tell the caller to publish again, so that the later cycle decides whether the human is asked.

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
