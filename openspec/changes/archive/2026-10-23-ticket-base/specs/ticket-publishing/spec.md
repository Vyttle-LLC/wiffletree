## MODIFIED Requirements

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
