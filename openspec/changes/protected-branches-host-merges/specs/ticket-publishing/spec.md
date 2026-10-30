## RENAMED Requirements

- FROM: `### Requirement: The host never approves, merges, closes or comments`
- TO: `### Requirement: The host never approves, closes or comments, and merges only through merge_ticket`

## MODIFIED Requirements

### Requirement: The host never approves, closes or comments, and merges only through merge_ticket
The host SHALL offer agents no operation that approves, closes, reopens, comments on, labels or reviews a pull request, or that edits an open pull request other than its title and description through `publish_ticket`. The host SHALL merge a pull request only through `merge_ticket`, and SHALL change an open pull request's base only to retarget a stacked ticket's pull request after a merge it made (see ticket-merging). Merging into a protected branch SHALL remain the human's, including a plan's feature branch into the default branch.

#### Scenario: No merge tool
- **WHEN** an agent lists the workspace tools
- **THEN** only `merge_ticket` merges a pull request, and none of them approves, closes or comments on one

#### Scenario: Into a protected branch
- **WHEN** the coordinator calls `merge_ticket` for a ticket whose pull request opens into `main`
- **THEN** it is refused and nothing is merged

### Requirement: Instructions state the publishing rules in Wiffletree's own words
The bundled instructions for every role, the fixed core with every skill at stock (see skills), SHALL NOT name an open-pr skill or any other skill for pull requests. The human's own rules and rewrites MAY name their own PR rules or skill, and the host SHALL NOT filter them. The fixed core SHALL tell agents never to push, or to create, edit, comment on, review, close or merge a pull request, from their shell; to publish with `publish_ticket` once the ticket's review has ended; that the workspace runs the push without asking when the human allowed the repository's owner and the review passed or the coordinator waived a blocked review, and otherwise only after the human approves each step; to write the pull request's title and description by the repository's and the human's own PR rules; that only the coordinator merges, with `merge_ticket`, and only into a branch that is not protected; and that only the human merges into a protected branch. The bundled instructions SHALL NOT say that any agent may merge from its shell. The coordinator's core SHALL say when to publish, that after a blocked cycle it publishes with `waived` under the same rules as accepting with a waiver, and that publishing without `waived` hands the decision to the human. The implementer's core SHALL tell it to write the title and description when asked to publish.

#### Scenario: No pull request skill
- **WHEN** the host builds the instructions for any role with every skill at stock
- **THEN** they do not contain "open-pr" and they name `publish_ticket`

#### Scenario: The human's own PR rules
- **WHEN** the human's rules for Implementing say "Follow my open-pr skill"
- **THEN** implementers get that rule as written, and the core's publishing rules are unchanged

#### Scenario: Merging
- **WHEN** the host builds the instructions for the project coordinator
- **THEN** the core says it merges only with `merge_ticket` into a branch that is not protected, and that only the human merges into a protected branch

#### Scenario: Other roles
- **WHEN** the host builds the instructions for an implementer
- **THEN** the core says never to merge a pull request from the shell and gives it no case in which it may merge

### Requirement: The limit is documented
The README and the development notes SHALL say that Wiffletree pushes a ticket's branch and opens its pull request only through this checked operation, without asking for owners the human allows and after the human's approval elsewhere; that it merges a ticket's pull request only through `merge_ticket`, only into a branch that is not protected and only in an allowed owner's repository; that it never merges into a protected branch and never approves, closes or comments on a pull request; and that agents run with their provider's permission prompts bypassed, so an agent's shell can still push, merge or call GitHub with the human's credentials, which these operations do not prevent.

#### Scenario: README
- **WHEN** a reader opens the README's warning and its "Humans own integration" paragraph
- **THEN** neither says Wiffletree never pushes, opens or merges pull requests, the paragraph says the human merges into protected branches, and the warning says agents can still run any command, including a push or a merge
