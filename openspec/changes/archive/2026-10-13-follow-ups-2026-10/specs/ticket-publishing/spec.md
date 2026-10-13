## MODIFIED Requirements

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
