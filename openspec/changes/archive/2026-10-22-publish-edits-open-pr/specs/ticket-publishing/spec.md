## MODIFIED Requirements

### Requirement: The host publishes a ticket on request
The host SHALL offer agents one tool, `publish_ticket`, taking a ticket id, an optional title and description, and an optional one-line `waived` reason. The host itself SHALL run the push and open the pull request; the tool SHALL NOT take a branch, a remote or a base. It SHALL push the ticket's own branch to the repository's `origin`. When the ticket has no open pull request, it SHALL then open one from that branch into the repository's configured base branch, and the title and description SHALL be required. When the ticket has an open pull request, it SHALL push, and when a title, a description or both are given it SHALL then change those of the open pull request with `gh pr edit`, validated as when opening one. Without either it SHALL only push. When GitHub's branch, read again before the push, is already at the commit, the host SHALL push nothing and SHALL only edit.

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

### Requirement: The host never approves, merges, closes or comments
The host SHALL offer agents no operation that approves, merges, closes, reopens, comments on, labels or reviews a pull request, or that edits an open pull request other than its title and description through `publish_ticket`. Merging SHALL remain the human's, including into a plan's feature branch.

#### Scenario: No merge tool
- **WHEN** an agent lists the workspace tools
- **THEN** none of them merges, approves, closes or comments on a pull request
