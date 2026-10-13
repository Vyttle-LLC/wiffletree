## MODIFIED Requirements

### Requirement: Progress in the sidebar tree
The project row in the sidebar tree SHALL show a thin bar beneath its label and the project's percentage beside it, and an archived project's row SHALL show neither bar nor percentage. Ticket rows and agent rows SHALL show neither a bar nor a progress figure, and SHALL be single-line rows whose height does not depend on progress; a ticket row with a pull request keeps its second line for the pull request. A project with nothing to show SHALL keep an empty track and no figure, so the project row's height never depends on its progress. Notes that are not progress, such as an agent's "held" or "queued input" and its questions figure, SHALL stay on the agent row.

#### Scenario: Rows under one project
- **WHEN** a project at 56% has a ticket Verifying at 70% with a finished implementer, a tester at 3 of 5 and a reviewer working without a plan
- **THEN** the project row shows "56%" and its bar, and the ticket row and the three agent rows show no figure and no bar

#### Scenario: Blocked ticket row
- **WHEN** a ticket is blocked at 26%
- **THEN** its sidebar row shows no "blocked" figure and no bar, and the strip and Crew panel still show its blocked state

#### Scenario: Project with nothing to show
- **WHEN** a project has no tickets and no progress to report
- **THEN** its row keeps an empty track and no figure, at the same height as other projects' rows

#### Scenario: Idle agent with no plan
- **WHEN** an agent is ready and its latest run had no plan
- **THEN** its row shows no figure and no track, at the same height as every other agent's and ticket's row

#### Scenario: Working agent and open ticket stay one line
- **WHEN** an agent is working at 3 of 5, or a ticket without a pull request is at 70%
- **THEN** its row has the same single-line height as an idle agent's, with no figure and no track line
