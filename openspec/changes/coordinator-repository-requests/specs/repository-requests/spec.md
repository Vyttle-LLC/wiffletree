## ADDED Requirements

### Requirement: A coordinator asks for a repository by name

The workspace SHALL offer `request_repository` to the project coordinator only, with `repository`, an `owner/name` or an absolute folder path, and `reason`, one short line. Any other caller SHALL be refused.

#### Scenario: A repository the project does not use

- **WHEN** a coordinator requests a workspace repository its project does not use
- **THEN** an Approval item is raised in the Inbox
- **AND** the tool returns that it is waiting for the human

#### Scenario: A folder not in the workspace

- **WHEN** a coordinator requests an absolute path that is a Git repository root and is not in the workspace
- **THEN** an Approval item is raised for that folder, canonicalized

#### Scenario: A worker asks

- **WHEN** an implementer, tester or reviewer calls `request_repository`
- **THEN** the call is refused and no item is raised

### Requirement: Nothing reaches the Inbox without cause

A request that needs no decision SHALL raise no item and SHALL say why.

#### Scenario: Already used

- **WHEN** the project already uses the repository, by name or by folder
- **THEN** the tool returns that it does, and no item is raised

#### Scenario: Not cloned

- **WHEN** an `owner/name` is not in the workspace
- **THEN** the call is refused, telling the coordinator to clone it and ask again with the folder path, and no item is raised

#### Scenario: Not a repository root

- **WHEN** a path does not exist, is not a repository root or has no commit
- **THEN** the call is refused with the reason, and no item is raised

#### Scenario: Several clones

- **WHEN** an `owner/name` matches more than one workspace repository the project does not use
- **THEN** the call is refused with their folders, and no item is raised

#### Scenario: Asked again while waiting

- **WHEN** the same repository is requested while its item waits
- **THEN** the tool returns the waiting state, and no second item is raised

#### Scenario: Asked again after a denial

- **WHEN** a denied repository is requested again
- **THEN** a new item is raised

### Requirement: Approving changes one project only

Approving SHALL add the repository to the project that asked and, for a folder, to the workspace. It SHALL change no Space's list and SHALL NOT move the project. The repository SHALL be usable as soon as the coordinator resumes.

#### Scenario: Listed by another Space

- **WHEN** the human approves a repository listed only by another Space
- **THEN** `project_repositories` includes it and `create_ticket` works there
- **AND** every Space's list and the project's Space are unchanged

#### Scenario: A folder under a followed root

- **WHEN** the human approves a folder under a root a Space follows
- **THEN** the repository is in the workspace and the project, and that Space's list is unchanged

### Requirement: An answer always reaches the coordinator

The answer, any change and the coordinator's message SHALL commit together, and the message SHALL resume the coordinator.

#### Scenario: Denied

- **WHEN** the human denies a request
- **THEN** nothing changes and the coordinator receives a message saying so

#### Scenario: Approval can no longer succeed

- **WHEN** the human approves after the folder has gone or changed, or the repository was removed
- **THEN** nothing changes and the coordinator receives a message saying why

#### Scenario: The coordinator was archived

- **WHEN** the human approves a request whose coordinator is archived
- **THEN** nothing changes and the message saying why is delivered when it is restored

### Requirement: A waiting request survives a restart

A waiting request SHALL be kept with its Inbox item.

#### Scenario: Answered after a restart

- **WHEN** the host restarts while a request waits and the human then approves it
- **THEN** the repository is added and the coordinator is told

### Requirement: The card shows only what the host knows

The Inbox card SHALL show the coordinator's name, the Approval pill, the title, the reason, the repository or folder, Listed by and This project, then Deny and **Add to this project**. The reason SHALL render as plain text and never as an action. The card SHALL offer no way to move a project or change a Space.

#### Scenario: Listed by no Space

- **WHEN** no Space lists the repository
- **THEN** Listed by reads No Space

### Requirement: Refusals point at the request

Refusing a repository the project does not use SHALL tell a coordinator to use `request_repository`.

#### Scenario: A ticket in an unused repository

- **WHEN** `create_ticket` names a repository the project does not use
- **THEN** the refusal names `request_repository`
