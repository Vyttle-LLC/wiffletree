## MODIFIED Requirements

### Requirement: Enforced hierarchy
The host SHALL allow projects without repositories, require each project to belong to one Space, require each ticket to reference a repository its project uses, and require workers to belong to the project coordinator of the same project and to work on one of its tickets. Multiple tickets SHALL share a repository. Assistant threads and job runs SHALL be the only sessions outside projects: each SHALL belong to one Space, and a job run to its job; they SHALL have no parent session, no ticket and no worktree, and SHALL NOT be offered the coordination tools (`create_ticket`, `assign_ticket`, `send_message`, `report`, `schedule` and the others).

#### Scenario: Invalid worker owner
- **WHEN** a worker is created under a session from another project, or a repository coordinator is created
- **THEN** the host rejects creation without persisting a partial assignment

#### Scenario: A thread is not a worker
- **WHEN** an assistant thread tries to call `create_ticket` or `report`
- **THEN** the tool is not offered to it, and no ticket or report is created

#### Scenario: A project without a Space
- **WHEN** a project is created without a Space
- **THEN** the host puts it in the Space last used for a new project, and rejects the creation when no Space exists
