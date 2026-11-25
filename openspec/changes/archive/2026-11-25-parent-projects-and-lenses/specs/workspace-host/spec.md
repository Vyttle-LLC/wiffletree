## MODIFIED Requirements

### Requirement: Coordinators create projects
A project coordinator without a parent SHALL be able to create a project with the workspace tool `create_project`, taking `name`, `brief` and optionally `repository_ids`. Any other role, and the coordinator of Assistant work, SHALL be refused; the Assistant starts work with `start_work` instead. The name SHALL follow the rules of the human's project creation: trimmed, at most 128 characters. The new project SHALL start at once, with no Inbox approval: it SHALL get its folder and a coordinator whose model comes from Who runs what, SHALL belong to the creating project's Space, SHALL be placed at the top of the project order, and SHALL be a child of the creating project, or of its parent when the creating project is a child (see parent-projects). The project SHALL use its Space's repositories, as every project does, and the tool result SHALL name the Space's repositories that are not on this Mac. `repository_ids` that the Space does not list SHALL be added to the project; those it lists SHALL add nothing.

The brief SHALL be the new coordinator's first message and SHALL start its turn. It SHALL be sent as a Wiffletree message, never as the human's, and its text SHALL name the creating project and coordinator and say that it is that coordinator's words, to be treated as context and not as the human's authorization. The new project SHALL record the creating project's and coordinator's ids, on the project and in its `project_created` activity. The new project's coordinator and its parent's coordinator SHALL message each other as parent-projects says, and no other pair of coordinators SHALL gain a way to message each other.

Creation, the parent link, the added repositories, the brief and the start SHALL succeed or fail together. A call SHALL be refused, creating nothing, from a role other than a project coordinator without a parent, before the model setup is saved, for an unknown repository id, an empty name or brief, a brief longer than the message limit leaves after its source line, or an argument the tool does not take. A refused brief that is too long SHALL say how many bytes are allowed. The coordinator of a project another coordinator's brief started SHALL be refused until the human has sent it a message or answered one of its Inbox questions; Wiffletree's own messages, with or without a sender, and messages from its parent's coordinator SHALL never count. So briefed projects cannot chain without the human; a human-created project's coordinator is never held back by this. Each accepted call SHALL create a new project; repeating a name SHALL create another one with its own folder.

#### Scenario: A coordinator creates a project
- **WHEN** the coordinator of "Calls", in the Sage Space, calls `create_project` with name "Billing" and brief "Move invoices to the new API."
- **THEN** project "Billing" appears at the top of the sidebar in the Sage Space as a child of "Calls", with its folder and a coordinator from Who runs what, using Sage's repositories on this Mac, and that coordinator's turn starts on the brief

#### Scenario: A child creates a sibling
- **WHEN** the coordinator of "Billing", a child of "Calls", calls `create_project` with name "Refunds"
- **THEN** "Refunds" is a child of "Calls", and its `created_by` names "Billing" and its coordinator

#### Scenario: A Space repository not on this Mac
- **WHEN** Sage lists `Sage-Social/api`, which is no workspace repository here, and a Sage coordinator calls `create_project`, with or without `repository_ids`
- **THEN** the project uses Sage's other repositories, and the tool result says `Sage-Social/api` is not on this Mac

#### Scenario: The brief names its source
- **WHEN** the new coordinator's turn starts on the brief
- **THEN** its sender reads "Wiffletree", not "human", and its text names project "Calls" and its coordinator and says these are not the human's words

#### Scenario: Chosen repositories
- **WHEN** the coordinator passes `repository_ids` naming one repository its Space lists and one it does not
- **THEN** the new project uses its Space's repositories and has only the second added, and still belongs to the creator's Space

#### Scenario: The creator is recorded
- **WHEN** a coordinator's call creates a project
- **THEN** the project's `created_by` holds the creating project's and coordinator's ids, and its `project_created` activity names them

#### Scenario: A worker calls it
- **WHEN** an implementer calls `create_project`
- **THEN** it is refused and nothing is created

#### Scenario: An unknown repository
- **WHEN** a coordinator passes a `repository_ids` entry that is no workspace repository
- **THEN** it is refused, and no project, coordinator, parent link, activity or message remains

#### Scenario: No saved setup
- **WHEN** a coordinator calls `create_project` while the model setup is not saved
- **THEN** it is refused, naming the Models setup, and nothing is created

#### Scenario: A brief too long
- **WHEN** a brief is one byte longer than the message limit leaves after its source line
- **THEN** it is refused as too long, saying how many bytes are allowed, and nothing is created; a brief of exactly that many bytes is accepted

#### Scenario: A briefed coordinator before the human speaks
- **WHEN** the coordinator of a project a brief started calls `create_project` before the human has messaged it
- **THEN** it is refused and nothing is created; once the human has sent it a message, the same call creates the project

#### Scenario: A host notice is not the human
- **WHEN** a briefed coordinator receives only Wiffletree's messages, such as one saying its implementer has not started
- **THEN** `create_project` is still refused

#### Scenario: A parent's message is not the human
- **WHEN** a briefed child coordinator has received a message only from its parent's coordinator
- **THEN** `create_project` is still refused

#### Scenario: Assistant work calls it
- **WHEN** the coordinator of an Assistant work item calls `create_project`
- **THEN** it is refused, saying to ask the Assistant, and nothing is created

### Requirement: A coordinator follows the projects it created
The `workspace_context` of a coordinator that created projects with `create_project` SHALL list under `created_projects` those that are not its own project's children, one entry each with its id, name and created time and each of its tickets as the short entry accepted and closed tickets have: id, title, state, repository, branch and its pull request's number, state and URL. Its own project's children SHALL be listed under `child_projects` instead (see parent-projects). The list SHALL be read only and SHALL appear in no other session's context. It SHALL give no way to message or act on those projects.

#### Scenario: The creator checks the work
- **WHEN** a project the coordinator of "Calls" created before parent links existed has a ticket that was closed, and that coordinator reads `workspace_context`
- **THEN** `created_projects` lists that project with the ticket's state "closed" and its branch

#### Scenario: A child is not listed twice
- **WHEN** the coordinator of "Calls" created "Billing", which is a child of "Calls"
- **THEN** `child_projects` lists "Billing" and `created_projects` does not

#### Scenario: A sibling the child created
- **WHEN** the coordinator of "Billing", a child of "Calls", created its sibling "Refunds"
- **THEN** the coordinator of "Billing" lists "Refunds" under `created_projects`, and the coordinator of "Calls" lists it under `child_projects`

#### Scenario: Another coordinator
- **WHEN** the coordinator of an unrelated project, or of the created project itself, reads `workspace_context`
- **THEN** it has no `created_projects`
