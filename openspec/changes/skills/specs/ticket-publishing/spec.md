## MODIFIED Requirements

### Requirement: Instructions state the publishing rules in Wiffletree's own words
The bundled instructions for every role, the fixed core with every skill at stock (see skills), SHALL NOT name an open-pr skill or any other skill for pull requests. The human's own rules and rewrites MAY name their own PR rules or skill, and the host SHALL NOT filter them. The fixed core SHALL tell agents never to push, or to create, edit, comment on, review, close or merge a pull request, from their shell; to publish with `publish_ticket` once the ticket's review has ended; that the workspace runs the push without asking when the review passed and the human allowed the repository's owner, and otherwise only after the human approves each step; that a review that ended blocked always needs that approval; to write the pull request's title and description by the repository's and the human's own PR rules; and that only the human merges. The bundled instructions SHALL NOT say that a coordinator may merge into a plan's feature branch. The coordinator's core SHALL say when to publish and to publish after a blocked cycle only where it would otherwise accept with a waiver. The implementer's core SHALL tell it to write the title and description when asked to publish.

#### Scenario: No pull request skill
- **WHEN** the host builds the instructions for any role with every skill at stock
- **THEN** they do not contain "open-pr" and they name `publish_ticket`

#### Scenario: The human's own PR rules
- **WHEN** the human's rules for Implementing say "Follow my open-pr skill"
- **THEN** implementers get that rule as written, and the core's publishing rules are unchanged

#### Scenario: Merging
- **WHEN** the host builds the instructions for the project coordinator
- **THEN** the core says only the human merges and gives the coordinator no case in which it may merge
