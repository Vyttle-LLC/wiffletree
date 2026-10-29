# ticket-publishing Specification

## Purpose
Let agents ask the host to push a ticket's branch and open or update its pull request, with the host checking the review, the branch and the repository's owner before anything leaves the machine, and asking the human wherever its rules require.

## Requirements

### Requirement: The host publishes a ticket on request
The host SHALL offer agents one tool, `publish_ticket`, taking a ticket id, an optional title and description, and an optional one-line `waived` reason. The host itself SHALL run the push and open the pull request; the tool SHALL NOT take a branch, a remote or a base. It SHALL push the ticket's own branch to the repository's `origin`. When the ticket has no open pull request, it SHALL then open one from that branch into the ticket's `base` if it has one, otherwise into the repository's configured base branch, and the title and description SHALL be required. When the ticket has an open pull request, it SHALL push, and when a title, a description or both are given it SHALL then change those of the open pull request with `gh pr edit`, validated as when opening one. Without either it SHALL only push. When GitHub's branch, read again before the push, is already at the commit, the host SHALL push nothing and SHALL only edit.

#### Scenario: First publish
- **WHEN** `publish_ticket` is called with a title and description for a reviewed ticket with no pull request
- **THEN** the host pushes the ticket's branch and opens a pull request into the repository's base branch with that title and description

#### Scenario: Update
- **WHEN** `publish_ticket` is called for a ticket whose pull request is open
- **THEN** the host pushes the ticket's branch and opens nothing

#### Scenario: Edit the open pull request
- **WHEN** `publish_ticket` is called with a title, a description or both for a ticket whose pull request is open
- **THEN** the host pushes any new commit, then changes only the given fields of that pull request, and the result says whether the push landed when the edit fails

#### Scenario: Edit without new commits
- **WHEN** `publish_ticket` is called with a description for a ticket whose open pull request's branch on GitHub is already at the commit
- **THEN** the host pushes nothing and only edits the pull request

#### Scenario: Missing title
- **WHEN** `publish_ticket` is called without a title for a ticket with no open pull request
- **THEN** it is refused and nothing is sent

#### Scenario: A ticket on another base
- **WHEN** `publish_ticket` is called for a reviewed ticket created with `base` `release` and no pull request
- **THEN** the pull request opens into `release`

### Requirement: Only the ticket's coordinator or implementer may publish
`publish_ticket` SHALL be accepted only from the project coordinator that owns the ticket or from an implementer assigned to it. A tester, a reviewer, or an agent of another ticket SHALL be refused. Only the coordinator SHALL pass `waived` (see "The coordinator's waiver publishes without asking").

#### Scenario: Implementer publishes
- **WHEN** the ticket's implementer calls `publish_ticket` for its ticket
- **THEN** the call is accepted, subject to the other checks

#### Scenario: Verifier refused
- **WHEN** a reviewer on the ticket calls `publish_ticket`
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
When the ticket's repository does not publish without asking, the host SHALL NOT push, open a pull request or edit one until the human approves that step in the Inbox. Each push, each pull request and each edit SHALL be its own approval, including every later push to the same pull request. The host SHALL write the item: it SHALL name the repository and whether it is public, the branch, the commit and its size, and how the review ended, and for a pull request the base branch and the full title and description, and for an edit the new title or description in full. The item for a first push, of a branch the host has not pushed and that has had no pull request, SHALL say the branch is not on GitHub yet. The item for a push to an open pull request SHALL say in its title how many new commits it sends. A push item's card SHALL say what follows the push as the rules stand while it waits: that opening or editing the pull request is a separate approval only while that step would ask, and otherwise, as once the repository's owner has been allowed, that the pull request is then opened or edited without asking again. Approving SHALL run exactly that step for exactly what the item showed: that repository, branch, base branch, commit, title and description. If any of them is no longer current when the human answers or when the step is about to run, such as a changed base, a changed `origin` or a moved HEAD, the host SHALL send nothing, SHALL withdraw the item and SHALL tell the caller what changed. Denying SHALL send nothing and SHALL tell the caller so. The item SHALL be an approval the human can only approve or deny: the human SHALL NOT be able to dismiss it and no agent SHALL be able to close or answer it. The request SHALL survive a host restart. While an item waits, the same publish requested again SHALL return the waiting status, and one requested with a newer commit or a different title or description SHALL withdraw the old item and raise a new one. When the ticket is accepted or closed while an item waits, the host SHALL withdraw the item, SHALL send nothing and SHALL tell the caller the request was withdrawn and why. After a request has ended, by denial, failure or interruption, a publish requested again SHALL raise a new item, also for the same step and commit.

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
The host SHALL offer agents no operation that approves, merges, closes, reopens, comments on, labels or reviews a pull request, or that edits an open pull request other than its title and description through `publish_ticket`. Merging SHALL remain the human's, including into a plan's feature branch.

#### Scenario: No merge tool
- **WHEN** an agent lists the workspace tools
- **THEN** none of them merges, approves, closes or comments on a pull request

### Requirement: Instructions state the publishing rules in Wiffletree's own words
The bundled instructions for every role, the fixed core with every skill at stock (see skills), SHALL NOT name an open-pr skill or any other skill for pull requests. The human's own rules and rewrites MAY name their own PR rules or skill, and the host SHALL NOT filter them. The fixed core SHALL tell agents never to push, or to create, edit, comment on, review, close or merge a pull request, from their shell; to publish with `publish_ticket` once the ticket's review has ended; that the workspace runs the push without asking when the human allowed the repository's owner and the review passed or the coordinator waived a blocked review, and otherwise only after the human approves each step; to write the pull request's title and description by the repository's and the human's own PR rules; and that only the human merges. The bundled instructions SHALL NOT say that a coordinator may merge into a plan's feature branch. The coordinator's core SHALL say when to publish, that after a blocked cycle it publishes with `waived` under the same rules as accepting with a waiver, and that publishing without `waived` hands the decision to the human. The implementer's core SHALL tell it to write the title and description when asked to publish.

#### Scenario: No pull request skill
- **WHEN** the host builds the instructions for any role with every skill at stock
- **THEN** they do not contain "open-pr" and they name `publish_ticket`

#### Scenario: The human's own PR rules
- **WHEN** the human's rules for Implementing say "Follow my open-pr skill"
- **THEN** implementers get that rule as written, and the core's publishing rules are unchanged

#### Scenario: Merging
- **WHEN** the host builds the instructions for the project coordinator
- **THEN** the core says only the human merges and gives the coordinator no case in which it may merge

### Requirement: The limit is documented
The README and the development notes SHALL say that Wiffletree pushes a ticket's branch and opens its pull request only through this checked operation, without asking for owners the human allows and after the human's approval elsewhere; that it never approves, merges, closes or comments on a pull request; and that agents run with their provider's permission prompts bypassed, so an agent's shell can still push or call GitHub with the human's credentials, which this operation does not prevent.

#### Scenario: README
- **WHEN** a reader opens the README's warning and its "Humans own integration" paragraph
- **THEN** neither says Wiffletree never pushes or opens pull requests, and the warning says agents can still run any command, including a push

### Requirement: The approval names the review level
A publish approval's Review row SHALL name the level of the ticket's latest verification cycle and SHALL say when that cycle was a re-check. When the latest cycle is a re-check, the approval SHALL also show a "Last full review" row naming the most recent passed full review of the ticket: its cycle, its level, the commit it verified and its verifiers. A cycle recorded without a level SHALL be shown without one and SHALL count as a full review. The level SHALL NOT change whether the human is asked: a passed re-check SHALL publish as any passed review does, and a review that ended blocked SHALL need the human's approval at every level.

#### Scenario: After a full review
- **WHEN** a push waits for approval and the ticket's latest cycle is cycle 1, a passed "Full" review by Tests, Correctness and Regressions at the commit being pushed
- **THEN** the item's Review row reads "Cycle 1 · Full passed at this commit · Tests, Correctness, Regressions", and it has no "Last full review" row

#### Scenario: After a re-check
- **WHEN** a push waits for approval, cycle 1 was a passed "Full" review at `9e03c1b`, and the latest cycle is cycle 2, a passed "Light" re-check by Correctness at the commit being pushed
- **THEN** the Review row reads "Cycle 2 · Light re-check passed at this commit · Correctness"
- **AND** a "Last full review" row reads "Cycle 1 · Full passed at 9e03c1b · Tests, Correctness, Regressions"

#### Scenario: A re-check that ended blocked
- **WHEN** `publish_ticket` is called in an allowed owner's repository for a ticket whose latest cycle, a "Light" re-check, ended blocked
- **THEN** nothing is sent, and the approval says the review ended blocked, names it as a Light re-check, lists the open findings and shows the "Last full review" row

#### Scenario: A passed re-check in an allowed owner's repository
- **WHEN** a ticket in an allowed owner's repository has an open pull request, its latest cycle is a passed re-check at HEAD, and `publish_ticket` is called
- **THEN** the host pushes the commit, and no Inbox item is raised

#### Scenario: A cycle from before levels
- **WHEN** a push waits for approval and the ticket's latest cycle passed and was recorded without a level
- **THEN** the Review row reads as before, "Cycle 1 passed at this commit" with its verifiers, and there is no "Last full review" row

### Requirement: The host may publish for the PR keeper
The host SHALL be able to start a publish of a ticket itself, for the PR keeper, without a `publish_ticket` call. Such a publish SHALL only push the ticket's branch to its open pull request; it SHALL NOT open a pull request and SHALL take no title or description. It SHALL be subject to every check, at every moment, that a publish requested with `publish_ticket` is: the ticket is open, reviewed and has no cycle running; the worktree is on the ticket's branch at the reviewed commit or a patch-equivalent one; `origin` is one checked place on GitHub that the base tracks; the frozen request is compared again before anything is sent; the owner is confirmed with GitHub; exactly one ref is sent to the frozen URL; and GitHub's branch is replaced only when it is the commit the host last pushed or was once this branch's tip. No rule SHALL be relaxed for the keeper. It SHALL carry the waiver recorded on the ticket, as a later `publish_ticket` call does.

A keeper publish SHALL be started only for a ticket whose publish would run without asking. It SHALL never raise an Inbox item: when a step would need the human's approval, as when the owner was unticked or a later review ended blocked, the request SHALL end with nothing sent. Its result SHALL go to the keeper's record of the ticket and SHALL NOT be sent to any agent as a publish result. While its step runs, `publish_ticket` for the ticket SHALL be refused as for any running step. A keeper publish that was running when the host stopped SHALL be ended at the next start with nothing more sent, and the next pass SHALL decide anew.

#### Scenario: A rebased branch is pushed for the keeper
- **WHEN** the keeper has moved a patch-equivalent rebase onto a ticket's branch in an allowed owner's repository whose latest cycle passed, and GitHub's branch is at the commit the host last pushed
- **THEN** the host replaces GitHub's branch with the rebased commit under a lease on that commit, raises no Inbox item, and sends no agent a publish result

#### Scenario: A rebase of a waived blocked review
- **WHEN** the keeper has moved a patch-equivalent rebase onto a ticket's branch in an allowed owner's repository whose latest cycle ended blocked and whose coordinator published it with a waiver
- **THEN** the host pushes the rebased commit and raises no Inbox item

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
- **WHEN** the ticket's latest cycle ended blocked and the ticket records no waiver
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

### Requirement: A stacked ticket's pull request opens into its parent's branch
A new pull request for a stacked ticket SHALL open into its parent's branch. `publish_ticket` SHALL refuse it, sending nothing and creating no request, until the host has pushed the parent's branch, and the refusal SHALL say to publish the parent first. Once the parent's pull request has merged, a new pull request for the child SHALL open into the parent's base instead: the parent's `base` if it has one, otherwise the repository's base branch. The size a publish reports SHALL be measured against the ticket's base. A ticket without a parent SHALL publish as before.

#### Scenario: Before the parent is pushed
- **WHEN** the coordinator publishes a stacked ticket whose parent was never published
- **THEN** the call is refused, saying to publish that ticket first, and no request exists

#### Scenario: After the parent is pushed
- **WHEN** the parent's branch was pushed and the child's review passed with one commit of its own
- **THEN** the request opens the pull request with `--base` the parent's branch and its size reads "1 commit · 1 file changed, …"

#### Scenario: The parent's pull request merged
- **WHEN** the parent's pull request has merged before the child is published
- **THEN** the child's pull request opens into the repository's base branch

#### Scenario: The parent is on another base
- **WHEN** the child of a ticket created on `release` is published, first after the parent was pushed and then after the parent's pull request merged
- **THEN** its pull request opens into the parent's branch, and then into `release`

### Requirement: The coordinator's waiver publishes without asking
`publish_ticket`'s `waived` reason SHALL be accepted only from the project coordinator that owns the ticket. After a blocked cycle it SHALL be accepted only under the conditions `accept_ticket` uses for a waiver: no cycle is running, every verifier of the latest cycle checked, none having reported that it could not verify or never reported, and no ledger entry is untriaged; the host SHALL make this check with the same code for both tools. Once the ticket has used every full review the cycle cap allows, the waiver MAY also cover the commits past the verified one that it names by short SHA (see ticket-workspaces); below the cap, or for commits it does not name, nothing changes. After a passed cycle a waiver that covers no such commit SHALL be refused. The request SHALL record the reason and any commits it covers.

In an allowed owner's repository, a request carrying the coordinator's waiver SHALL run its steps without asking the human, except opening another pull request after an earlier one was closed or merged. Where the owner is not allowed, each step SHALL ask as any request does, and its item SHALL show the waiver's reason and any unreviewed commits. The tool's result SHALL say that the publish runs under the coordinator's waiver.

When such a request is made, the host SHALL record the waiver on the ticket with the commits it covers. A later publish of the same review, by the coordinator or the implementer, SHALL stand on the recorded waiver without passing `waived` again, and so SHALL the PR keeper's push. Starting a new verification cycle SHALL clear it.

#### Scenario: Allowed owner, waived blocked review
- **WHEN** the ticket's latest cycle ended blocked with F4 open, every verifier checked, nothing is untriaged, and its coordinator calls `publish_ticket` with `waived` "F4 needs a fetched base the host always has" in an allowed owner's repository
- **THEN** the host pushes the branch and opens the pull request, no Inbox item is raised, and the result says it is publishing under the coordinator's waiver

#### Scenario: Allowed owner, waived commits at the cap
- **WHEN** at the cap the coordinator calls `publish_ticket` with `waived` naming the one commit after the reviewed one, in an allowed owner's repository
- **THEN** the host publishes without asking, and the request and the ticket record that commit

#### Scenario: The implementer passes waived
- **WHEN** the ticket's implementer calls `publish_ticket` with `waived`
- **THEN** it is refused and nothing is sent

#### Scenario: A finding is untriaged
- **WHEN** the coordinator passes `waived` after a blocked cycle whose ledger has untriaged F6
- **THEN** it is refused, naming F6, and nothing is sent

#### Scenario: A verifier could not verify
- **WHEN** the coordinator passes `waived` after a blocked cycle in which a verifier reported that it could not verify
- **THEN** it is refused, saying a waiver covers only checked findings, and nothing is sent

#### Scenario: Owner not allowed
- **WHEN** the coordinator publishes a waived blocked review in a repository whose owner is not allowed
- **THEN** the push and the pull request each wait for their own approval, and each item shows the waiver's reason

#### Scenario: A later publish of the same review
- **WHEN** after a waived publish the implementer calls `publish_ticket` without `waived` to change the open pull request's description
- **THEN** the host publishes without asking

#### Scenario: A new cycle
- **WHEN** a new verification cycle starts on the ticket
- **THEN** the recorded waiver is cleared, and a publish after that cycle is decided on it alone

### Requirement: A blocked review without a waiver needs the human's approval
When the ticket's latest verification cycle ended blocked and the request carries no waiver of the coordinator's, the host SHALL NOT push or open a pull request without the human's approval in the Inbox, in every repository, including an allowed owner's. The item SHALL say that the review ended blocked, SHALL name any verifier that could not check, and SHALL list every open finding, which is every finding in the ticket's ledger that is neither fixed nor settled as follow-up or won't-fix, with its id, severity, verifier, location and summary. In an allowed owner's repository there SHALL be one such approval for the commit, naming everything that would be sent, and approving it SHALL run the commit's steps without a further item. Elsewhere the notice and the findings SHALL appear on each step's approval. The approval SHALL cover that commit only.

#### Scenario: Blocked review in an allowed owner's repository
- **WHEN** `publish_ticket` is called without `waived` for a ticket in an allowed owner's repository whose latest cycle ended blocked with open findings F4 and F6
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
- **WHEN** a later commit's cycle also ends blocked and `publish_ticket` is called without `waived`
- **THEN** the human is asked again for that commit
