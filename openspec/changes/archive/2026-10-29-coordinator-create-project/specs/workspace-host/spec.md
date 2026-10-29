## ADDED Requirements

### Requirement: Coordinators create projects
A project coordinator SHALL be able to create a project with the workspace tool `create_project`, taking `name`, `brief` and optionally `repository_ids`. Any other role SHALL be refused. The name SHALL follow the rules of the human's project creation: trimmed, at most 128 characters. The new project SHALL start at once, with no Inbox approval: it SHALL get its folder and a coordinator whose model comes from Who runs what, and SHALL be placed at the top of the project order. `repository_ids` SHALL set its repository selection as `set_project_repositories` does; without it the project SHALL use every workspace repository.

The brief SHALL be the new coordinator's first message and SHALL start its turn. It SHALL be sent as a Wiffletree message, never as the human's, and its text SHALL name the creating project and coordinator and say that it is that coordinator's words, to be treated as context and not as the human's authorization. The new project SHALL record the creating project's and coordinator's ids, on the project and in its `project_created` activity. The two coordinators SHALL NOT gain a way to message each other.

Creation, the repository selection, the brief and the start SHALL succeed or fail together. A call SHALL be refused, creating nothing, from a role other than project coordinator, before the model setup is saved, for an unknown repository id, an empty name or brief, a brief longer than the message limit leaves after its source line, or an argument the tool does not take. A refused brief that is too long SHALL say how many bytes are allowed. The coordinator of a project another coordinator's brief started SHALL be refused until the human has sent it a message, so briefed projects cannot chain without the human; a human-created project's coordinator is never held back by this. Each accepted call SHALL create a new project; repeating a name SHALL create another one with its own folder.

#### Scenario: A coordinator creates a project
- **WHEN** the coordinator of "Calls" calls `create_project` with name "Billing" and brief "Move invoices to the new API."
- **THEN** project "Billing" appears at the top of the sidebar with its folder and a coordinator from Who runs what, using every workspace repository, and that coordinator's turn starts on the brief

#### Scenario: The brief names its source
- **WHEN** the new coordinator's turn starts on the brief
- **THEN** its sender reads "Wiffletree", not "human", and its text names project "Calls" and its coordinator and says these are not the human's words

#### Scenario: Chosen repositories
- **WHEN** the coordinator passes `repository_ids` naming one workspace repository
- **THEN** the new project uses only that repository

#### Scenario: The creator is recorded
- **WHEN** a coordinator's call creates a project
- **THEN** the project's `created_by` holds the creating project's and coordinator's ids, and its `project_created` activity names them

#### Scenario: A worker calls it
- **WHEN** an implementer calls `create_project`
- **THEN** it is refused and nothing is created

#### Scenario: An unknown repository
- **WHEN** a coordinator passes a `repository_ids` entry that is no workspace repository
- **THEN** it is refused, and no project, coordinator, activity or message remains

#### Scenario: No saved setup
- **WHEN** a coordinator calls `create_project` while the model setup is not saved
- **THEN** it is refused, naming the Models setup, and nothing is created

#### Scenario: A brief too long
- **WHEN** a brief is one byte longer than the message limit leaves after its source line
- **THEN** it is refused as too long, saying how many bytes are allowed, and nothing is created; a brief of exactly that many bytes is accepted

#### Scenario: A briefed coordinator before the human speaks
- **WHEN** the coordinator of a project a brief started calls `create_project` before the human has messaged it
- **THEN** it is refused and nothing is created; once the human has sent it a message, the same call creates the project

### Requirement: A coordinator follows the projects it created
The `workspace_context` of a coordinator that created projects with `create_project` SHALL list them under `created_projects`, one entry each with its id, name and created time and each of its tickets as the short entry accepted and closed tickets have: id, title, state, repository, branch and its pull request's number, state and URL. The list SHALL be read only and SHALL appear in no other session's context. It SHALL give no way to message or act on those projects.

#### Scenario: The creator checks the work
- **WHEN** the project the coordinator of "Calls" created has a ticket that was closed, and that coordinator reads `workspace_context`
- **THEN** `created_projects` lists that project with the ticket's state "closed" and its branch

#### Scenario: Another coordinator
- **WHEN** the coordinator of an unrelated project, or of the created project itself, reads `workspace_context`
- **THEN** it has no `created_projects`

## MODIFIED Requirements

### Requirement: Host messages come from Wiffletree
Messages the host sends without a sender, such as a verification round's or cycle's result, a kept-worktree notice, a migration notice, a PR watcher message or the brief a coordinator's `create_project` hands the new coordinator, SHALL be labelled as coming from Wiffletree: the recipient's turn input SHALL name the sender "Wiffletree", or "Wiffletree PR watcher" for the PR watcher, and the conversation view SHALL show them as Wiffletree notices. Messages the human sends, including answers to inbox items, SHALL keep the human label.

#### Scenario: Verification result
- **WHEN** a verification cycle ends and the coordinator's next turn includes its result
- **THEN** that message's sender reads "Wiffletree", not "human", and the chat shows it as a Wiffletree notice

#### Scenario: A project brief
- **WHEN** a coordinator's `create_project` delivers its brief to the new coordinator
- **THEN** that message's sender reads "Wiffletree", not "human"

#### Scenario: The human's message
- **WHEN** the human sends the coordinator a message
- **THEN** its sender still reads "human" and the chat shows it as the human's
