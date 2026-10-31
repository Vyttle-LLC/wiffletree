## MODIFIED Requirements

<!-- Delta order for "Enforced hierarchy": 1 spaces → 2 assistant-spaces-jobs-boards (this) → 3 brain-compile. This text restates spaces' version and names the roles this change admits outside projects. -->

### Requirement: Enforced hierarchy
The host SHALL allow projects without repositories, require each project to belong to one Space, require each ticket to reference a repository its project uses, and require workers to belong to the project coordinator of the same project and to work on one of its tickets. Multiple tickets SHALL share a repository. Every session SHALL have one owner: a project, a Space or a brain. A session owned by a Space or a brain SHALL have no parent session and no ticket; one owned by a Space SHALL have no worktree, and one owned by a brain SHALL work only in a worktree of that brain's host checkout. No such session SHALL be offered the coordination tools (`create_ticket`, `assign_ticket`, `send_message`, `report`, `schedule` and the others). Only the roles this requirement names SHALL run outside projects: assistant threads, issue threads and job runs, each owned by one Space, a job run also belonging to its job. The host SHALL NOT create a project record to hold sessions that are not project work.

#### Scenario: Invalid worker owner
- **WHEN** a worker is created under a session from another project, or a repository coordinator is created
- **THEN** the host rejects creation without persisting a partial assignment

#### Scenario: A project without a Space
- **WHEN** a project is created without a Space
- **THEN** the host puts it in the Space last used for a new project, and rejects the creation when no Space exists

#### Scenario: A worker outside a project
- **WHEN** a command creates an implementer owned by a Space
- **THEN** the host rejects creation without persisting anything

#### Scenario: A thread is not a worker
- **WHEN** an assistant thread or an issue thread tries to call `create_ticket` or `report`
- **THEN** the tool is not offered to it, and no ticket or report is created
