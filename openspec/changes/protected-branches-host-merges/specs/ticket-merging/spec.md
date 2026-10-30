## ADDED Requirements

### Requirement: The coordinator merges a ticket's pull request with merge_ticket
The host SHALL offer one tool, `merge_ticket`, taking a ticket id and an optional one-line `waived` reason. The host itself SHALL run the merge; the tool SHALL NOT take a pull request, a branch, a base or a method. It SHALL be accepted only from the project coordinator that owns the ticket. An implementer, a tester, a reviewer, the PR keeper and an agent of another ticket SHALL be refused.

#### Scenario: The implementer asks
- **WHEN** a ticket's implementer calls `merge_ticket`
- **THEN** it is refused, saying only the project coordinator can merge a ticket, and nothing is merged

### Requirement: The host never merges into a protected branch
The host SHALL refuse `merge_ticket`, merging nothing, when the base of the ticket's pull request on GitHub is protected (see protected-branches). No Inbox answer, waiver or instruction SHALL override this refusal, and the host SHALL raise no Inbox item for it. The host SHALL read GitHub's default branch, branch protection and rulesets for that base at the time of each merge, and SHALL refuse the merge when any of those reads fails.

#### Scenario: Into main
- **WHEN** the coordinator calls `merge_ticket` for the ticket "Spaces into main", whose pull request opens into `main`, the default branch
- **THEN** it is refused, saying `main` is protected and only the human merges into it, no Inbox item is raised, and nothing is merged

#### Scenario: A branch a ruleset protects
- **WHEN** a ticket's pull request opens into `release/2026.11`, which no entry matches and a GitHub ruleset protects
- **THEN** it is refused, naming the ruleset

#### Scenario: GitHub cannot be read
- **WHEN** the read of the base's rulesets fails
- **THEN** it is refused, saying GitHub could not be read, and nothing is merged

### Requirement: The checks before a merge
Before merging or asking the human, the host SHALL refuse `merge_ticket` unless all of these hold: the ticket is open, has a pull request the watcher recorded as open and has no merge request running or waiting; the ticket could be accepted with the same `waived` reason, by the same code `accept_ticket` uses; the repository publishes without asking (see repository-owners); the pull request, read again from GitHub, opens into the ticket's base; its base is not protected; its head is the commit the host last pushed for the ticket and the worktree's HEAD, and is the reviewed commit or a patch-equivalent one, as `publish_ticket` checks; it is not a draft; GitHub reports it mergeable, with no conflict and not blocked; and its checks have not failed and it has at least one check. A pull request behind its base SHALL NOT be refused for that. A refusal SHALL say why and SHALL merge nothing. No answer from the human SHALL override these checks.

#### Scenario: An owner that is not allowed
- **WHEN** the coordinator merges a ticket in `celadon-theme/website` and `celadon-theme` is not allowed
- **THEN** it is refused, saying the owner is not allowed and the human merges there

#### Scenario: A head that is not reviewed
- **WHEN** the pull request's head is `5f2e7a0` and the reviewed commit is `c81f04d` with another patch
- **THEN** it is refused, naming both, and saying to verify again

#### Scenario: A conflict
- **WHEN** GitHub reports that the pull request conflicts with its base
- **THEN** it is refused, saying to resolve the conflict and publish again

#### Scenario: Failed checks
- **WHEN** the pull request's checks failed
- **THEN** it is refused, saying the checks failed

#### Scenario: No checks
- **WHEN** the pull request has no checks at all
- **THEN** it is refused, saying the repository runs no checks on it, so the human merges it

#### Scenario: Acceptance would be refused
- **WHEN** the ticket's ledger has untriaged F6
- **THEN** it is refused, naming F6, and nothing is merged

#### Scenario: Behind its base
- **WHEN** the pull request is behind `feature/spaces` with no conflict and passing checks after a passed review
- **THEN** it is merged

### Requirement: A merge waits for running checks
When every check holds except that the pull request's checks are still running, the host SHALL record a waiting merge on the ticket with its head, and SHALL decide it again after each PR watcher pass of the repository. It SHALL merge once the checks have passed and every check still holds. It SHALL end the request, merging nothing, and tell the coordinator once, when the checks fail, the pull request closes or merges otherwise, a conflict appears, the base becomes protected, a new verification cycle starts, or the head becomes a commit the host did not push with the same change. When the host itself pushes a new head with the same change, such as the PR keeper's rebase, the request SHALL follow that head and wait for its checks. Accepting or closing the ticket SHALL withdraw it.

#### Scenario: Checks pass later
- **WHEN** the coordinator merges a ticket whose checks are running, and the next pass finds them passed
- **THEN** the pull request is merged at its head, the ticket is accepted, and the coordinator gets one message saying so

#### Scenario: Checks fail later
- **WHEN** a waiting merge's checks fail
- **THEN** nothing is merged, the request ends, and the coordinator is told once

#### Scenario: The keeper rebases while it waits
- **WHEN** the PR keeper pushes a rebase with the same change while a merge waits
- **THEN** the request waits for the new head's checks and merges that head

### Requirement: A merge after a blocked review always asks the human
A merge SHALL run without asking only when the ticket's latest verification cycle passed and the request needs no waiver. When the latest cycle ended blocked, whether or not the coordinator passed `waived`, and when the head stands on a waiver of commits no review checked, the host SHALL merge nothing until the human approves one Inbox item. The item SHALL name the repository and whether it is public, the branches and that the base is not protected, the merge method and head, the review with how it ended and its verifiers, the waiver's reason if given, and the findings still open. The host SHALL make every check again when the human approves and immediately before it merges; when one fails, it SHALL merge nothing and tell the coordinator. Approval SHALL cover that head only. When the checks are still running after the approval, the merge SHALL wait for them without asking again. This SHALL apply in every repository, including an allowed owner's after a waived blocked review, which publishes without asking.

#### Scenario: A waived blocked review
- **WHEN** the ticket's latest cycle ended blocked with F4 open, every verifier checked, nothing is untriaged, and the coordinator calls `merge_ticket` with `waived` in an allowed owner's repository
- **THEN** nothing is merged, one Inbox item asks whether to merge although the review ended blocked, showing the waiver's reason and F4, and the tool answers that it waits for the human

#### Scenario: The human approves
- **WHEN** the human approves that item and the head, checks and base are unchanged
- **THEN** the pull request is merged at that head and the ticket is accepted with the waiver

#### Scenario: The head moved before the answer
- **WHEN** the human approves after the pull request's head moved
- **THEN** nothing is merged and the coordinator is told to verify and merge again

#### Scenario: The human denies
- **WHEN** the human denies the item
- **THEN** nothing is merged, and the coordinator gets one message saying so

### Requirement: Only the host creates merge approvals
Only the host SHALL be able to create an Inbox item that is drawn or handled as a merge approval. An agent's request to raise an Inbox item under the identifier prefix the host uses for merge approvals SHALL be refused, and no item SHALL be created. The host SHALL merge on an approval only when the answered item is the one recorded for the ticket's waiting merge request, and SHALL take the ticket and the head from that stored request.

#### Scenario: A coordinator tries to create a merge approval
- **WHEN** the coordinator asks the human with an operation id under the merge prefix
- **THEN** it is refused and no item is created

### Requirement: The merge method follows GitHub
The host SHALL merge with the first of squash, merge commit and rebase that GitHub reports the repository allows, read at the time of the merge, with `gh pr merge <number> --<method> --match-head-commit <head>` and GitHub's default title and message. It SHALL NOT pass `--auto`, `--admin` or `--delete-branch`.

#### Scenario: Squash allowed
- **WHEN** a repository allows squash and merge commits
- **THEN** the merge is a squash, pinned to the checked head

#### Scenario: Only merge commits
- **WHEN** a repository allows only merge commits
- **THEN** the merge is a merge commit

#### Scenario: The head moves at the last moment
- **WHEN** the head moves between the last check and the merge
- **THEN** GitHub refuses the merge, nothing is merged, and the coordinator is told

### Requirement: The merge accepts the ticket
After GitHub merges the pull request, the host SHALL record the pull request as merged, with its merge commit and that the host merged it, and SHALL accept the ticket by the code `accept_ticket` uses, with the coordinator's `waived` reason, or, after a blocked review without one, the reason "Merged after a blocked review with the human's approval". When acceptance fails after the merge, the ticket SHALL stay open and the coordinator SHALL be told that the pull request merged and why the ticket was not accepted. A pull request the human merges on GitHub SHALL be handled as before (see pr-watch).

#### Scenario: Accepted
- **WHEN** the host merges a passed ticket's pull request #180
- **THEN** the ticket is accepted, its agents are archived, its worktree is removed and its branch is kept

#### Scenario: Acceptance fails after the merge
- **WHEN** an untracked file appears in the worktree between the checks and the merge
- **THEN** the pull request is merged, the ticket stays open, and the coordinator is told it merged and why it was not accepted

### Requirement: Stacked tickets follow a merge the host made
After the host merges a ticket, for each open ticket stacked on it whose pull request is open into the merged ticket's branch, the host SHALL change that pull request's base on GitHub to the merged pull request's base, and SHALL send nothing for one GitHub already retargeted. The result SHALL name each child and say whether the PR keeper will rebase it. A child with no pull request SHALL be left as it is. The host SHALL NOT delete the merged branch.

#### Scenario: A stacked child
- **WHEN** the host merges "Space storage" into `feature/spaces`, and "Space tracker", stacked on it, has pull request #181 open into `wiffletree/space-storage`
- **THEN** #181's base becomes `feature/spaces`, and the result names "Space tracker"

#### Scenario: GitHub deleted the branch
- **WHEN** the repository deletes merged branches and GitHub already moved #181 to `feature/spaces`
- **THEN** no base change is sent

### Requirement: Merge results reach the coordinator from Wiffletree
The tool SHALL answer at once that the pull request merged, that it waits for checks, that it waits for the human, or why it was refused. The outcome of a waiting request SHALL reach the coordinator as one message from Wiffletree. A merged result SHALL name the ticket, the pull request, the base, the method, the head and the merge commit, say whether the ticket was accepted, and name each stacked child retargeted. Messages SHALL carry no pull request title, description or comment text.

#### Scenario: Merged at once
- **WHEN** the host merges #180 into `feature/spaces` with squash at `7c41a09`
- **THEN** the coordinator's result names #180, `feature/spaces`, squash, `7c41a09` and the merge commit, and says the ticket was accepted

### Requirement: A merge interrupted by a restart is finished or ended
When the host starts with a merge that was running when it stopped, or whose `gh pr merge` stopped at its time limit, it SHALL read the pull request: merged at the recorded head SHALL be finished as a merge the host made; still open SHALL end the request with nothing more sent, and the coordinator SHALL be told once to call `merge_ticket` again. A waiting merge SHALL survive a restart.

#### Scenario: Stopped while merging
- **WHEN** the host stops after `gh pr merge` was sent and starts again, and GitHub reports the pull request merged at the recorded head
- **THEN** the ticket is accepted and the coordinator gets the merged result once

### Requirement: The coordinator is told when to merge
The bundled coordinator instructions SHALL say that it merges a ticket's pull request into a branch that is not protected with `merge_ticket` once the ticket's review passed and its pull request is published; that the merge accepts the ticket; that a merge after a blocked review always waits for the human; and that only the human merges into a protected branch, including a feature branch into the default branch. The communication contract SHALL say that agents never merge a pull request from their shell.

#### Scenario: Coordinator instructions
- **WHEN** the host builds the coordinator's instructions with every skill at stock
- **THEN** they name `merge_ticket` and say only the human merges into a protected branch
