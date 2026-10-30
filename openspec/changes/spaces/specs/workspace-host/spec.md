## MODIFIED Requirements

<!-- Delta order for "Enforced hierarchy": 1 spaces (this) → 2 assistant-spaces-jobs-boards → 3 brain-compile. Each later delta restates this text with its own addition. -->

### Requirement: Enforced hierarchy
The host SHALL allow projects without repositories, require each project to belong to one Space, require each ticket to reference a repository its project uses, and require workers to belong to the project coordinator of the same project and to work on one of its tickets. Multiple tickets SHALL share a repository. Every session SHALL have one owner: a project, a Space or a brain. A session owned by a Space or a brain SHALL have no parent session, no ticket and no worktree, and SHALL NOT be offered the coordination tools (`create_ticket`, `assign_ticket`, `send_message`, `report`, `schedule` and the others). Only the roles this requirement names SHALL run outside projects, and it names none. The host SHALL NOT create a project record to hold sessions that are not project work.

#### Scenario: Invalid worker owner
- **WHEN** a worker is created under a session from another project, or a repository coordinator is created
- **THEN** the host rejects creation without persisting a partial assignment

#### Scenario: A project without a Space
- **WHEN** a project is created without a Space
- **THEN** the host puts it in the Space last used for a new project, and rejects the creation when no Space exists

#### Scenario: A worker outside a project
- **WHEN** a command creates an implementer owned by a Space
- **THEN** the host rejects creation without persisting anything

<!-- Delta order for "Coordinators create projects": 1 spaces (this). -->

### Requirement: Coordinators create projects
A project coordinator SHALL be able to create a project with the workspace tool `create_project`, taking `name`, `brief` and optionally `repository_ids`. Any other role SHALL be refused. The name SHALL follow the rules of the human's project creation: trimmed, at most 128 characters. The new project SHALL start at once, with no Inbox approval: it SHALL get its folder and a coordinator whose model comes from Who runs what, SHALL belong to the creating project's Space, and SHALL be placed at the top of the project order. `repository_ids` SHALL set its repository selection as `set_project_repositories` does; without it the project SHALL use the workspace repositories whose GitHub name is one of its Space's repositories, and the tool result SHALL name the Space's repositories that are not on this Mac.

The brief SHALL be the new coordinator's first message and SHALL start its turn. It SHALL be sent as a Wiffletree message, never as the human's, and its text SHALL name the creating project and coordinator and say that it is that coordinator's words, to be treated as context and not as the human's authorization. The new project SHALL record the creating project's and coordinator's ids, on the project and in its `project_created` activity. The two coordinators SHALL NOT gain a way to message each other.

Creation, the repository selection, the brief and the start SHALL succeed or fail together. A call SHALL be refused, creating nothing, from a role other than project coordinator, before the model setup is saved, for an unknown repository id, an empty name or brief, a brief longer than the message limit leaves after its source line, or an argument the tool does not take. A refused brief that is too long SHALL say how many bytes are allowed. The coordinator of a project another coordinator's brief started SHALL be refused until the human has sent it a message or answered one of its Inbox questions; Wiffletree's own messages, with or without a sender, SHALL never count. So briefed projects cannot chain without the human; a human-created project's coordinator is never held back by this. Each accepted call SHALL create a new project; repeating a name SHALL create another one with its own folder.

#### Scenario: A coordinator creates a project
- **WHEN** the coordinator of "Calls", in the Sage Space, calls `create_project` with name "Billing" and brief "Move invoices to the new API."
- **THEN** project "Billing" appears at the top of the sidebar in the Sage Space with its folder and a coordinator from Who runs what, using Sage's repositories on this Mac, and that coordinator's turn starts on the brief

#### Scenario: A Space repository not on this Mac
- **WHEN** Sage lists `Sage-Social/api`, which is no workspace repository here, and a Sage coordinator calls `create_project` without `repository_ids`
- **THEN** the project uses Sage's other repositories, and the tool result says `Sage-Social/api` is not on this Mac

#### Scenario: The brief names its source
- **WHEN** the new coordinator's turn starts on the brief
- **THEN** its sender reads "Wiffletree", not "human", and its text names project "Calls" and its coordinator and says these are not the human's words

#### Scenario: Chosen repositories
- **WHEN** the coordinator passes `repository_ids` naming one workspace repository
- **THEN** the new project uses only that repository, and still belongs to the creator's Space

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

#### Scenario: A host notice is not the human
- **WHEN** a briefed coordinator receives only Wiffletree's messages, such as one saying its implementer has not started
- **THEN** `create_project` is still refused
