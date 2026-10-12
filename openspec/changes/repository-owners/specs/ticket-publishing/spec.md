## Purpose

Let agents ask the host to push a ticket's branch and open or update its pull request, with the host checking the review, the branch and the repository's owner before anything leaves the machine, and asking the human wherever its rules require.

## ADDED Requirements

### Requirement: The host publishes a ticket on request
The host SHALL offer agents one tool, `publish_ticket`, taking a ticket id and an optional title and description. The host itself SHALL run the push and open the pull request; the tool SHALL NOT take a branch, a remote or a base. It SHALL push the ticket's own branch to the repository's `origin`. When the ticket has no open pull request, it SHALL then open one from that branch into the repository's configured base branch, and the title and description SHALL be required. When the ticket has an open pull request, it SHALL only push, and SHALL refuse a title or description.

#### Scenario: First publish
- **WHEN** `publish_ticket` is called with a title and description for a reviewed ticket with no pull request
- **THEN** the host pushes the ticket's branch and opens a pull request into the repository's base branch with that title and description

#### Scenario: Update
- **WHEN** `publish_ticket` is called for a ticket whose pull request is open
- **THEN** the host pushes the ticket's branch and opens nothing

#### Scenario: Missing title
- **WHEN** `publish_ticket` is called without a title for a ticket with no open pull request
- **THEN** it is refused and nothing is sent

### Requirement: Only the ticket's coordinator or implementer may publish
`publish_ticket` SHALL be accepted only from the project coordinator that owns the ticket or from an implementer assigned to it. A tester, a reviewer, or an agent of another ticket SHALL be refused.

#### Scenario: Implementer publishes
- **WHEN** the ticket's implementer calls `publish_ticket` for its ticket
- **THEN** the call is accepted, subject to the other checks

#### Scenario: Verifier refused
- **WHEN** a reviewer on the ticket calls `publish_ticket`
- **THEN** it is refused and nothing is sent

### Requirement: Checks before anything leaves the machine
Before pushing or asking the human, the host SHALL refuse a publish unless all of these hold: the ticket is open; the ticket has a verification cycle and none is running; the worktree's HEAD is the commit that cycle's last round checked, or a patch-equivalent one such as a squash or a clean rebase; the worktree is on the ticket's branch; and the repository's `origin` is on `github.com`, still names the repository the host recorded for it, and is the remote its base tracks. A refusal SHALL say why and SHALL send nothing. No answer from the human SHALL override these checks. The host SHALL make the same checks again, for the same commit, when the human approves a step.

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

#### Scenario: HEAD moves while a card waits
- **WHEN** the human approves a push after the worktree's HEAD has moved to different changes
- **THEN** nothing is pushed and the caller is told

### Requirement: The host never overwrites commits it did not push
The host SHALL record the commit it last pushed for each ticket. It SHALL push a ticket's branch only when GitHub's branch is absent, is behind the commit being pushed, or is at the commit the host last pushed, in which case it MAY replace it, as after a squash or a clean rebase. Otherwise it SHALL refuse and push nothing.

#### Scenario: New branch
- **WHEN** GitHub has no branch for the ticket
- **THEN** the host creates it at the reviewed commit

#### Scenario: Rebased after the host's push
- **WHEN** GitHub's branch is at the commit the host last pushed and the worktree was cleanly rebased
- **THEN** the host replaces GitHub's branch with the rebased commit

#### Scenario: Someone else's commits
- **WHEN** GitHub's branch has a commit the host did not push and the worktree does not contain
- **THEN** the host refuses, pushes nothing, and says the branch has commits Wiffletree did not push

### Requirement: An allowed owner's repository publishes without asking after a passed review
When the ticket's latest verification cycle passed and its repository publishes without asking (see repository-owners), the host SHALL run every step of the publish without asking the human.

#### Scenario: Allowed owner, passed review
- **WHEN** `publish_ticket` is called for a ticket in an allowed owner's repository whose latest cycle passed
- **THEN** the host pushes the branch and opens the pull request, and no Inbox item is raised

#### Scenario: Allowed owner, later push
- **WHEN** a fix passes a new cycle and `publish_ticket` is called again
- **THEN** the host pushes the new commit without asking

### Requirement: Elsewhere the human approves each step
When the ticket's repository does not publish without asking, the host SHALL NOT push or open a pull request until the human approves that step in the Inbox. Each push and each pull request SHALL be its own approval, including every later push to the same pull request. The host SHALL write the item: it SHALL name the repository and whether it is public, the branch, the commit and its size, and how the review ended, and for a pull request the base branch and the full title and description. Approving SHALL run exactly that step for exactly that commit. Denying SHALL send nothing and SHALL tell the caller so. The item SHALL be an approval the human can only approve or deny: the human SHALL NOT be able to dismiss it and no agent SHALL be able to close or answer it. The request SHALL survive a host restart. A publish requested again for the same commit SHALL return the waiting status, and one requested for a newer commit SHALL withdraw the old item and raise a new one.

#### Scenario: Push waits for approval
- **WHEN** `publish_ticket` is called for a reviewed ticket in a repository whose owner is not allowed
- **THEN** nothing is pushed, the tool returns that the push waits for the human, and the Inbox shows an approval naming the repository, its visibility, the branch and the commit

#### Scenario: The pull request is a second approval
- **WHEN** the human approves the push and the ticket has no pull request
- **THEN** the host pushes the branch, and a second approval shows the pull request's base branch, title and description in full before it is opened

#### Scenario: A later push asks again
- **WHEN** a fix passes review and `publish_ticket` is called for a ticket whose pull request is open in a repository whose owner is not allowed
- **THEN** a new approval is raised for that push

#### Scenario: Denied
- **WHEN** the human denies the push
- **THEN** nothing is sent and the caller gets a message saying so

#### Scenario: A coordinator's word is not approval
- **WHEN** the coordinator tells an implementer the human agreed, and the implementer calls `publish_ticket`
- **THEN** the step still waits for the human's own answer in the Inbox

#### Scenario: Approval after a restart
- **WHEN** the host restarts while a push waits for approval and the human then approves it
- **THEN** the host makes its checks and pushes

#### Scenario: Newer commit
- **WHEN** a push for commit A waits for approval and `publish_ticket` is called after commit B passed review
- **THEN** the item for A is withdrawn and a new one is raised for B

### Requirement: A blocked review always needs the human's approval
When the ticket's latest verification cycle ended blocked, the host SHALL NOT push or open a pull request without the human's approval in the Inbox, in every repository, including an allowed owner's. The item SHALL say that the review ended blocked, SHALL name any verifier that could not check, and SHALL list every open finding, which is every finding in the ticket's ledger that is neither fixed nor settled as follow-up or won't-fix, with its id, severity, verifier, location and summary. In an allowed owner's repository there SHALL be one such approval for the commit, naming everything that would be sent, and approving it SHALL run the commit's steps without a further item. Elsewhere the notice and the findings SHALL appear on each step's approval. The approval SHALL cover that commit only.

#### Scenario: Blocked review in an allowed owner's repository
- **WHEN** `publish_ticket` is called for a ticket in an allowed owner's repository whose latest cycle ended blocked with open findings F4 and F6
- **THEN** nothing is sent, and the Inbox shows one approval saying the review ended blocked and listing F4 and F6 with their severity, location and summary

#### Scenario: Approved despite the blocked review
- **WHEN** the human approves that item for a ticket with no pull request
- **THEN** the host pushes the branch and opens the pull request without a second item

#### Scenario: Blocked review where the owner is not allowed
- **WHEN** `publish_ticket` is called for such a ticket in a repository whose owner is not allowed
- **THEN** the push approval and the pull request approval each say the review ended blocked and list the open findings

#### Scenario: A verifier could not check
- **WHEN** the blocked cycle includes a verifier that reported it could not verify
- **THEN** the item names that verifier as not having checked

#### Scenario: The next commit
- **WHEN** a later commit's cycle also ends blocked and `publish_ticket` is called
- **THEN** the human is asked again for that commit

### Requirement: Results arrive as messages from Wiffletree
`publish_ticket` SHALL return at once with a refusal, the steps the host is running, or the step that waits for the human and why. When the steps have finished, failed or been denied, the host SHALL send the caller one message, labelled as coming from Wiffletree, that starts the caller's next turn. A success SHALL name the branch, the commit, the repository and the pull request's number and URL. A failure SHALL carry the reason and SHALL say what, if anything, was sent. A step interrupted by a host restart SHALL be reported to the caller as interrupted, and calling `publish_ticket` again SHALL be safe.

#### Scenario: Success
- **WHEN** the host has pushed a ticket's branch and opened PR #45
- **THEN** the caller gets one message from Wiffletree naming the branch, the commit, the repository and PR #45 with its URL

#### Scenario: Push fails
- **WHEN** `git push` fails because the host has no credentials for the remote
- **THEN** the caller gets a message with Git's error, saying nothing was pushed

#### Scenario: Waiting status
- **WHEN** a step waits for the human
- **THEN** the tool's result says which step waits and why, and tells the caller to finish its turn

#### Scenario: Interrupted by a restart
- **WHEN** the host stops while a push is running and starts again
- **THEN** the caller is told the publish was interrupted, and a second `publish_ticket` call neither pushes a different commit nor opens a second pull request

### Requirement: A waiting step shows on the ticket
While a publish step waits for the human, the ticket's sidebar row SHALL show a line saying which step waits for approval. The line SHALL go when the step is approved, denied or withdrawn.

#### Scenario: Waiting line
- **WHEN** a ticket's push waits for the human's approval
- **THEN** its row shows that the push waits for approval

### Requirement: The host never approves, merges, closes or comments
The host SHALL offer agents no operation that approves, merges, closes, reopens, comments on, labels or reviews a pull request, or that edits an open pull request's title or description. Merging SHALL remain the human's, including into a plan's feature branch.

#### Scenario: No merge tool
- **WHEN** an agent lists the workspace tools
- **THEN** none of them merges, approves, closes or comments on a pull request

### Requirement: Instructions state the publishing rules in Wiffletree's own words
The bundled instructions for every role SHALL NOT name an open-pr skill or any other skill for pull requests. They SHALL tell agents never to push, or to create, edit, comment on, review, close or merge a pull request, from their shell; to publish with `publish_ticket` once the ticket's review has ended; that the workspace runs the push without asking when the review passed and the human allowed the repository's owner, and otherwise only after the human approves each step; that a review that ended blocked always needs that approval; to write the pull request's title and description by the repository's and the human's own PR rules; and that only the human merges. They SHALL NOT say that a coordinator may merge into a plan's feature branch. The coordinator's instructions SHALL say when to publish and to publish after a blocked cycle only where it would otherwise accept with a waiver. The implementer's instructions SHALL tell it to write the title and description when asked to publish.

#### Scenario: No pull request skill
- **WHEN** the host builds the instructions for any role
- **THEN** they do not contain "open-pr" and they name `publish_ticket`

#### Scenario: Merging
- **WHEN** the host builds the instructions for the project coordinator
- **THEN** they say only the human merges and give the coordinator no case in which it may merge

### Requirement: The limit is documented
The README and the development notes SHALL say that Wiffletree pushes a ticket's branch and opens its pull request only through this checked operation, without asking for owners the human allows and after the human's approval elsewhere; that it never approves, merges, closes or comments on a pull request; and that agents run with their provider's permission prompts bypassed, so an agent's shell can still push or call GitHub with the human's credentials, which this operation does not prevent.

#### Scenario: README
- **WHEN** a reader opens the README's warning and its "Humans own integration" paragraph
- **THEN** neither says Wiffletree never pushes or opens pull requests, and the warning says agents can still run any command, including a push
