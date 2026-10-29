## REMOVED Requirements

### Requirement: Ticket workspaces in the sidebar
**Reason**: The sidebar lists projects only; the Agents panel lists each project's tickets and their agents.
**Migration**: See "Ticket workspaces in the Agents panel" here, and "The sidebar lists projects only" in native-workspace.

## ADDED Requirements

### Requirement: Ticket workspaces in the Agents panel
The Agents panel SHALL show each counted ticket of a project as a card labelled "repository · ticket" with the repository name and ticket title, and under it the ticket's agents (see work-progress). A ticket accepted with a waiver SHALL say so in the warning color on its card, as "Accepted with waiver · N open findings", and SHALL never be shown only as passed or accepted. There SHALL be no repository-team row or "Add repository team" action.

#### Scenario: Card shape
- **WHEN** a project has a ticket "Fix inbound calls" in repository "sagechat-sms" with an implementer and two reviewers
- **THEN** the Agents panel shows a card "sagechat-sms · Fix inbound calls" with its stage, and beneath its bar the three agents

#### Scenario: Waived ticket
- **WHEN** a project has one passed ticket, one blocked ticket and one ticket accepted with a waiver and one open entry
- **THEN** the waived ticket's card reads "Accepted with waiver · 1 open finding", and the others' cards do not

#### Scenario: New ticket picks a repository
- **WHEN** the human opens New ticket for a project
- **THEN** the dialog asks for a repository the project uses, a title and a brief, and creates the ticket under that project's coordinator
