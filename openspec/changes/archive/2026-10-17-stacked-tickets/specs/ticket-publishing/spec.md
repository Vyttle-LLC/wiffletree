## ADDED Requirements

### Requirement: A stacked ticket's pull request opens into its parent's branch
A new pull request for a stacked ticket SHALL open into its parent's branch. `publish_ticket` SHALL refuse it, sending nothing and creating no request, until the host has pushed the parent's branch, and the refusal SHALL say to publish the parent first. Once the parent's pull request has merged, a new pull request for the child SHALL open into the repository's base branch instead. The size a publish reports SHALL be measured against the ticket's base. A ticket without a parent SHALL publish as before.

#### Scenario: Before the parent is pushed
- **WHEN** the coordinator publishes a stacked ticket whose parent was never published
- **THEN** the call is refused, saying to publish that ticket first, and no request exists

#### Scenario: After the parent is pushed
- **WHEN** the parent's branch was pushed and the child's review passed with one commit of its own
- **THEN** the request opens the pull request with `--base` the parent's branch and its size reads "1 commit · 1 file changed, …"

#### Scenario: The parent's pull request merged
- **WHEN** the parent's pull request has merged before the child is published
- **THEN** the child's pull request opens into the repository's base branch
