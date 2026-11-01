## ADDED Requirements

### Requirement: The Publishing Behavior can ask before every publish
When the Publishing Behavior resolves to "always ask first" for a ticket's repository in its project's Space (see settings-cascade), a `publish_ticket` request SHALL record it when it starts, and every step of that request SHALL wait for the human's approval as in a repository whose owner is not allowed, even in an allowed owner's repository after a passed review or under the coordinator's waiver. The status `publish_ticket` returns and the Inbox card SHALL say the step waits because the ticket's Publishing Behavior is "Always ask first". The PR keeper's own pushes SHALL keep their rules.

#### Scenario: Allowed owner, always ask
- **WHEN** `publish_ticket` is called for a passed ticket in an allowed owner's repository whose Space says "always ask first"
- **THEN** the push waits in the Inbox, its card says the owner is allowed but the Publishing Behavior is "Always ask first", and opening the pull request is a separate approval

#### Scenario: Default
- **WHEN** no layer sets Publishing
- **THEN** an allowed owner's passed ticket publishes without asking, as before
