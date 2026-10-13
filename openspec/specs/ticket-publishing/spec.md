# ticket-publishing Specification

## Purpose
Let agents ask the host to push a ticket's branch and open or update its pull request, with the host checking the review, the branch and the repository's owner before anything leaves the machine, and asking the human wherever its rules require.

## Requirements

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
Before pushing or asking the human, the host SHALL refuse a publish unless all of these hold: the ticket is open; the ticket has a verification cycle and none is running; the worktree's HEAD is the commit that cycle's last round checked, or a patch-equivalent one such as a squash or a clean rebase; the worktree is on the ticket's branch; and the repository's `origin` is on `github.com`, has exactly one push URL, has a fetch URL naming the same repository, still names the repository the host recorded for it, and is the remote its base tracks. A refusal SHALL say why and SHALL send nothing. No answer from the human SHALL override these checks. The host SHALL make the same checks again, for the same commit, when the human approves a step and immediately before it sends anything. A request SHALL record the verification cycle it was decided on and how that cycle ended. If the ticket's latest cycle is no longer that one at either of those moments, the host SHALL send nothing, SHALL end the request and SHALL tell the caller to publish again, so that the later cycle decides whether the human is asked.

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

### Requirement: The host never overwrites commits it did not push
The host SHALL record the commit it last pushed for each ticket. It SHALL push a ticket's branch only when GitHub's branch is absent, is behind the commit being pushed, or is at a commit the host may replace, as after a squash or a clean rebase. The host MAY replace GitHub's branch head only when it is the commit the host last pushed, or a commit that was once the tip of this ticket's branch in this repository on this machine, as the branch's reflog records, such as one an agent's shell pushed before the host published tickets. Otherwise it SHALL refuse and push nothing.

#### Scenario: New branch
- **WHEN** GitHub has no branch for the ticket
- **THEN** the host creates it at the reviewed commit

#### Scenario: Rebased after the host's push
- **WHEN** GitHub's branch is at the commit the host last pushed and the worktree was cleanly rebased
- **THEN** the host replaces GitHub's branch with the rebased commit

#### Scenario: Pushed from a shell before the host published tickets
- **WHEN** an agent's shell pushed the ticket's branch earlier, the host has recorded no push for the ticket, and the worktree was then cleanly rebased
- **THEN** the host replaces GitHub's branch with the rebased commit, because GitHub's head was once this branch's tip here

#### Scenario: Someone else's commits
- **WHEN** GitHub's branch has a commit the host did not push and the worktree does not contain
- **THEN** the host refuses, pushes nothing, and says the branch has commits Wiffletree did not push

### Requirement: Only the ticket's branch is pushed
A push by the host SHALL create or update exactly one ref at the destination, the ticket's branch, whatever the repository's or the user's Git configuration says. It SHALL NOT send tags, other branches or submodule commits, and SHALL NOT push to any destination other than the one push URL it checked. The host SHALL read GitHub's branch from, and push to, the push URL recorded in the request itself, never to a remote by name, so the destination cannot change between the check and the send.

#### Scenario: Tags are configured to follow
- **WHEN** Git is configured to push annotated tags along with commits and the reviewed commit has a tag GitHub does not have
- **THEN** the host pushes the ticket's branch and the tag is not sent

#### Scenario: Submodules are configured to be pushed
- **WHEN** Git is configured to push submodule commits on demand
- **THEN** the host pushes only the ticket's branch of the repository itself

#### Scenario: Other refspecs are configured
- **WHEN** the remote is configured to push all branches by default
- **THEN** the destination gains only the ticket's branch

#### Scenario: Origin is repointed after the check
- **WHEN** `origin`'s URL is changed to another repository after the host has checked the request and before Git sends anything
- **THEN** whatever is pushed goes to the URL the request recorded, and nothing reaches the new URL

### Requirement: The owner is confirmed with GitHub before anything is sent
Immediately before the host pushes or opens a pull request, it SHALL ask GitHub which owner and name the repository currently has and whether it is a fork, read-only, and SHALL compare the answer with the repository the request named. If they differ, the host SHALL send nothing, SHALL record the new answer, and SHALL tell the caller where the repository now lives; a publish requested after that SHALL be decided on the new owner. If GitHub gives no answer, the host SHALL send nothing and SHALL tell the caller why, whether or not the human approved the step.

#### Scenario: Transferred after it was allowed
- **WHEN** `allowed-org/app` was recorded yesterday, GitHub has since transferred it to `other-org`, its `origin` still has the old URL, and `publish_ticket` is called after a passed review
- **THEN** nothing is pushed, the repository is recorded under `other-org`, and the caller is told it moved

#### Scenario: Publishing again after the transfer
- **WHEN** `publish_ticket` is called again and `other-org` is not allowed
- **THEN** the push waits for the human's approval on an item naming `other-org/app`

#### Scenario: GitHub cannot be reached
- **WHEN** GitHub cannot be read when a push is about to run, even one the human approved
- **THEN** nothing is pushed and the caller is told GitHub could not confirm the repository

### Requirement: An allowed owner's repository publishes without asking after a passed review
When the ticket's latest verification cycle passed and its repository publishes without asking (see repository-owners), the host SHALL run every step of the publish without asking the human, except opening another pull request for a ticket whose earlier one was closed or merged (see below).

#### Scenario: Allowed owner, passed review
- **WHEN** `publish_ticket` is called for a ticket in an allowed owner's repository whose latest cycle passed
- **THEN** the host pushes the branch and opens the pull request, and no Inbox item is raised

#### Scenario: Allowed owner, later push
- **WHEN** a fix passes a new cycle and `publish_ticket` is called again
- **THEN** the host pushes the new commit without asking

### Requirement: Elsewhere the human approves each step
When the ticket's repository does not publish without asking, the host SHALL NOT push or open a pull request until the human approves that step in the Inbox. Each push and each pull request SHALL be its own approval, including every later push to the same pull request. The host SHALL write the item: it SHALL name the repository and whether it is public, the branch, the commit and its size, and how the review ended, and for a pull request the base branch and the full title and description. The item for a first push, of a branch the host has not pushed and that has had no pull request, SHALL say the branch is not on GitHub yet. The item for a push to an open pull request SHALL say in its title how many new commits it sends. A push item's card SHALL say what follows the push as the rules stand while it waits: that opening the pull request is a separate approval only while that step would ask, and otherwise, as once the repository's owner has been allowed, that the pull request is then opened without asking again. Approving SHALL run exactly that step for exactly what the item showed: that repository, branch, base branch, commit, title and description. If any of them is no longer current when the human answers or when the step is about to run, such as a changed base, a changed `origin` or a moved HEAD, the host SHALL send nothing, SHALL withdraw the item and SHALL tell the caller what changed. Denying SHALL send nothing and SHALL tell the caller so. The item SHALL be an approval the human can only approve or deny: the human SHALL NOT be able to dismiss it and no agent SHALL be able to close or answer it. The request SHALL survive a host restart. While an item waits, the same publish requested again SHALL return the waiting status, and one requested with a newer commit or a different title or description SHALL withdraw the old item and raise a new one. When the ticket is accepted or closed while an item waits, the host SHALL withdraw the item, SHALL send nothing and SHALL tell the caller the request was withdrawn and why. After a request has ended, by denial, failure or interruption, a publish requested again SHALL raise a new item, also for the same step and commit.

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

#### Scenario: The base changes while an approval waits
- **WHEN** a pull request approval shows base `main`, the repository's base is then changed to `origin/release`, and the human approves the item
- **THEN** no pull request is opened, the item is withdrawn, and the caller is told the base changed

#### Scenario: Asking again after a denial
- **WHEN** the human denies the pull request for commit C, the implementer corrects the description and calls `publish_ticket` again for C
- **THEN** a new approval is raised showing the corrected description

#### Scenario: Asking again after an interruption
- **WHEN** the human approved a push, the host stopped while it ran, and the caller calls `publish_ticket` again for the same commit
- **THEN** a new approval is raised for that push

#### Scenario: Newer commit
- **WHEN** a push for commit A waits for approval and `publish_ticket` is called after commit B passed review
- **THEN** the item for A is withdrawn and a new one is raised for B

#### Scenario: First push of a branch
- **WHEN** a push approval is raised for a ticket whose branch the host has never pushed and that has no pull request
- **THEN** the item's Branch row reads the branch followed by "not on GitHub yet"

#### Scenario: A later push names its commits
- **WHEN** a push approval is raised for one new commit to a ticket's open pull request in `celadon-theme/website`
- **THEN** the item's title reads "Push 1 new commit to celadon-theme/website?"

#### Scenario: The owner is allowed while the push waits
- **WHEN** a push waits for approval after a passed review, and the human then allows the repository's owner
- **THEN** the card no longer says that opening the pull request is a separate approval, and says the pull request is then opened without asking again
- **AND** approving the push opens the pull request without a second item

#### Scenario: The ticket is accepted or closed while a step waits
- **WHEN** a push waits for approval and the ticket is accepted or closed
- **THEN** the item is withdrawn, nothing is pushed, and the caller gets one message saying the request was withdrawn because the ticket was accepted or closed

### Requirement: Only the host creates publish approvals
Only the host SHALL be able to create an Inbox item that is drawn or handled as a publish approval. An agent's request to raise an Inbox item under the identifier prefix the host uses for publish approvals SHALL be refused, and no item SHALL be created. The prefix the host uses for provider permission approvals SHALL be refused in the same way, since such an item is also drawn as an approval. The host SHALL push or open a pull request on an approval only when the answered item is the one recorded for a ticket's waiting publish request, and SHALL take the ticket, the step and the commit from that stored request, never from anything an agent supplied.

#### Scenario: A coordinator tries to create a publish approval
- **WHEN** a coordinator calls `ask_user` with a request id beginning `publish:`
- **THEN** the call is refused, no Inbox item appears, and nothing is pushed

#### Scenario: A coordinator tries to create a permission approval
- **WHEN** a coordinator calls `ask_user` with a request id beginning `permission:`
- **THEN** the call is refused and no Inbox item appears

#### Scenario: An approval that no request names
- **WHEN** an Inbox item with a `publish:` identifier exists that no ticket's waiting publish request names, and it is answered "approved"
- **THEN** the host pushes nothing and opens nothing

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

### Requirement: Another pull request after a closed or merged one needs the human's approval
When the ticket's pull request, as the watcher last read it, was closed or merged, the host SHALL NOT open another pull request for the ticket without the human's approval in the Inbox, in every repository, including an allowed owner's after a passed review. The item SHALL name the earlier pull request and say whether it was closed or merged. The push before it SHALL follow the repository's ordinary rule. Where one approval already covers the commit's push and pull request, as after a blocked review in an allowed owner's repository, that item SHALL carry the same notice and no further item SHALL be raised.

#### Scenario: Closed pull request in an allowed owner's repository
- **WHEN** PR #45 of a ticket in an allowed owner's repository was closed without merging, a fix passes review and `publish_ticket` is called with a title and description
- **THEN** the host pushes the branch without asking, and opening the new pull request waits for an approval that says PR #45 was closed

#### Scenario: Merged pull request
- **WHEN** a ticket's pull request was merged and `publish_ticket` is called for further work on the ticket
- **THEN** no pull request is opened until the human approves an item that says the earlier one was merged

### Requirement: Results arrive as messages from Wiffletree
`publish_ticket` SHALL return at once with a refusal, the steps the host is running, or the step that waits for the human and why. Every accepted request SHALL end with exactly one message to its caller, labelled as coming from Wiffletree, that starts the caller's next turn: when its steps have finished or failed, or it was denied, withdrawn or interrupted. The human's answer to a publish approval SHALL NOT also be sent to the caller as an Inbox answer: the request's own message is the only one. A later request SHALL get its own message, also when it is for the same ticket and commit or comes from the ticket's other permitted caller. A success SHALL name the branch, the commit, the repository and the pull request's number and URL. It SHALL begin by saying the human approved both steps when the human approved the push and the pull request each on its own item, and SHALL NOT say so otherwise. A failure SHALL carry the reason and SHALL say what, if anything, was sent. A push that was stopped because it did not finish in time SHALL be reported as one that may or may not have reached GitHub, and a pull request step stopped the same way SHALL be reported as one that may or may not have opened the pull request, never as one that opened none. A step interrupted by a host restart SHALL be reported to the caller as interrupted, and calling `publish_ticket` again SHALL be safe.

#### Scenario: Success
- **WHEN** the host has pushed a ticket's branch and opened PR #45
- **THEN** the caller gets one message from Wiffletree naming the branch, the commit, the repository and PR #45 with its URL

#### Scenario: Denial is one message
- **WHEN** the human denies a push approval
- **THEN** the caller gets the request's one message from Wiffletree and no separate "Human answer" message

#### Scenario: Push fails
- **WHEN** `git push` fails because the host has no credentials for the remote
- **THEN** the caller gets a message with Git's error, saying nothing was pushed

#### Scenario: Retry after a failed push
- **WHEN** a push for commit C failed, the caller was told, the cause was fixed and `publish_ticket` is called again for C
- **THEN** the caller gets a second message with the new result

#### Scenario: The coordinator retries after the implementer
- **WHEN** the implementer's publish of commit C failed and the coordinator then publishes C
- **THEN** the coordinator gets the result of its own request

#### Scenario: Waiting status
- **WHEN** a step waits for the human
- **THEN** the tool's result says which step waits and why, and tells the caller to finish its turn

#### Scenario: Interrupted by a restart
- **WHEN** the host stops while a push is running and starts again
- **THEN** the caller is told the publish was interrupted, and a second `publish_ticket` call neither pushes a different commit nor opens a second pull request

#### Scenario: Success after both approvals
- **WHEN** the human approved the push and then the pull request, and the host opened PR #45
- **THEN** the caller's message begins "The human approved both steps."

#### Scenario: Success after one approval
- **WHEN** the human approved the push, the owner was allowed meanwhile, and the host opened the pull request without a second item
- **THEN** the caller's message does not say the human approved both steps

#### Scenario: Opening the pull request does not finish in time
- **WHEN** `gh pr create` is stopped at its time limit
- **THEN** the caller is told the branch was pushed and that the pull request may or may not have been opened, and that calling `publish_ticket` again is safe

### Requirement: A waiting step shows on the ticket
While a publish step waits for the human, the ticket's sidebar row SHALL show a line saying which step waits for approval. The line SHALL go when the step is approved, denied or withdrawn, including when the ticket is accepted or closed.

#### Scenario: Waiting line
- **WHEN** a ticket's push waits for the human's approval
- **THEN** its row shows that the push waits for approval

#### Scenario: The ticket is finished
- **WHEN** a ticket whose push waits for approval is accepted or closed
- **THEN** its row no longer shows that the push waits for approval

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
