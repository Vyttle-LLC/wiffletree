## MODIFIED Requirements

### Requirement: Enforced hierarchy
The host SHALL allow projects without repositories, require each ticket to reference a repository its project uses, and require workers to belong to the project coordinator of the same project and to work on one of its tickets. Multiple tickets SHALL share a repository. Each chosen tracker SHALL have one host-owned project of kind tracker, which SHALL have no coordinator, repositories or tickets and SHALL hold only issue assistant sessions; no other project SHALL hold an issue assistant, and a tracker project SHALL hold no other role.

#### Scenario: Invalid worker owner
- **WHEN** a worker is created under a session from another project, or a repository coordinator is created
- **THEN** the host rejects creation without persisting a partial assignment

#### Scenario: An assistant in an ordinary project
- **WHEN** a command creates an issue assistant session in a coordinator's project, or an implementer in a tracker project
- **THEN** the host rejects creation without persisting anything
