## MODIFIED Requirements

### Requirement: Ticket workspaces in the Agents panel
The Agents panel SHALL show each open ticket of a project, and each finished ticket with an unarchived agent, as a card labelled "repository · ticket" with the repository name and ticket title, and under it the ticket's agents, and every other finished ticket on a line in its "Finished tickets" section (see work-progress). A ticket accepted with a waiver SHALL say so in the warning color, under its card's bar or directly under its line, as "Accepted with waiver · N open findings", and SHALL never be shown only as passed or accepted. There SHALL be no repository-team row or "Add repository team" action.

#### Scenario: Card shape
- **WHEN** a project has a ticket "Fix inbound calls" in repository "sagechat-sms" with an implementer and two reviewers
- **THEN** the Agents panel shows a card "sagechat-sms · Fix inbound calls" with its stage, and beneath its bar the three agents

#### Scenario: Waived ticket
- **WHEN** a project has one passed ticket, one blocked ticket and one ticket accepted with a waiver and one open entry
- **THEN** the waived ticket's line in "Finished tickets" is followed by "Accepted with waiver · 1 open finding" in the warning color, and the other tickets show no such note

#### Scenario: New ticket picks a repository
- **WHEN** the human opens New ticket for a project
- **THEN** the dialog asks for a repository the project uses, a title and a brief, and creates the ticket under that project's coordinator
