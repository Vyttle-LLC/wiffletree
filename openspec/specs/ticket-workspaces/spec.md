# ticket-workspaces Specification

## Purpose
Coordinate a project through one project coordinator whose tickets each own a repository worktree, branch and agents.

## Requirements

### Requirement: Two-level coordination
A project SHALL have exactly one coordinator session, its project coordinator (role `project_orchestrator`). The project coordinator SHALL create tickets, assign agents to them, and accept or close them directly. The host SHALL NOT create sessions with role `task_orchestrator`, and the agent tool list SHALL NOT offer `create_repo_coordinator` or `archive_team`.

#### Scenario: Repository coordinator tools are gone
- **WHEN** any managed session lists its workspace tools
- **THEN** the list contains `create_ticket`, `assign_ticket`, `verify_ticket`, `accept_ticket` and `close_ticket`
- **AND** it contains neither `create_repo_coordinator` nor `archive_team`

#### Scenario: Only the project coordinator manages tickets
- **WHEN** an implementer, tester or reviewer calls `create_ticket`, `assign_ticket`, `verify_ticket`, `accept_ticket` or `close_ticket`
- **THEN** the host rejects the call without changing any ticket or session

#### Scenario: Legacy role cannot be created
- **WHEN** a client sends `CreateSession` with role `task_orchestrator`
- **THEN** the host rejects it and no session is stored

### Requirement: A ticket is a workspace
A ticket SHALL belong to one project coordinator and one repository that the project uses, and SHALL own exactly one worktree and one branch for its whole life. `create_ticket` SHALL take `title`, `brief` and `repository_id`, create the worktree and branch from the repository's base, and store `repository_id` on the ticket. Every agent assigned to the ticket SHALL work in that worktree, and no other operation SHALL create a worktree for it except re-creating the same path from the same branch on restore. Repeating `create_ticket` with the same coordinator, repository and title SHALL return the existing ticket.

#### Scenario: One worktree per ticket
- **WHEN** a single-repository, single-ticket task (SH-1171-sized) runs from `create_ticket` through implementation, verification by three verifiers and `accept_ticket`
- **THEN** exactly one worktree was registered for the ticket in its repository (`git worktree list` shows the ticket's `worktree` path and no other path on its `branch`)
- **AND** the implementer's and every verifier's runtime `workdir` equals the ticket's `worktree`

#### Scenario: Repository outside the project
- **WHEN** the project coordinator calls `create_ticket` with a `repository_id` the project does not use
- **THEN** the host rejects it, naming the repository, and creates no worktree or branch

#### Scenario: Repeated creation is idempotent
- **WHEN** `create_ticket` is called twice with the same title, brief and repository
- **THEN** the same ticket is returned and one worktree exists

#### Scenario: Several tickets share a repository
- **WHEN** the project coordinator creates two tickets in the same repository
- **THEN** each ticket has its own worktree and branch

### Requirement: Agents report directly to the project coordinator
Every agent assigned to a ticket SHALL have the ticket's project coordinator as its parent. Reports and messages SHALL travel between an agent and the project coordinator without any intermediate coordinator session, and a blocked report SHALL wake the project coordinator, not the human; only the project coordinator SHALL place questions in the human's inbox.

#### Scenario: No relay turns
- **WHEN** an SH-1171-sized task runs end to end, from the project coordinator's `create_ticket` through implementation, verification and `accept_ticket`
- **THEN** every stored message delivered to the implementer or to a verifier, including host-generated verification messages, has the project coordinator as its sender
- **AND** every report from the implementer or a verifier is addressed to the project coordinator
- **AND** every session with a stored provider run in the project has role `project_orchestrator`, `implementer`, `tester` or `reviewer`

#### Scenario: Blocked is not escalation
- **WHEN** an implementer reports `blocked`
- **THEN** the ticket becomes blocked and the project coordinator wakes with the report
- **AND** no attention item is created for the human until the project coordinator calls `ask_user`

### Requirement: Finishing a ticket
`accept_ticket` SHALL require the ticket's state to be `passed` and its worktree HEAD to equal the commit the last verification round verified. Accepting or closing a ticket SHALL archive the ticket's agents, keep their conversations and the branch, and remove the worktree, deferring removal while an agent is still in its turn and refusing while the worktree is locked or holds uncommitted or untracked files. Neither SHALL merge, push or publish.

#### Scenario: Commits after verification
- **WHEN** the project coordinator accepts a passed ticket whose worktree HEAD has moved past the verified commit
- **THEN** the host refuses, naming the verified and current commits

#### Scenario: Accepting archives the workspace
- **WHEN** the project coordinator accepts a passed ticket with a clean worktree and no agent in its turn
- **THEN** the ticket is `accepted`, its agents are archived with their conversations readable, its branch exists and its worktree path is gone

### Requirement: Ticket workspaces in the sidebar
The sidebar SHALL show each project as its coordinator, then its ticket workspaces, then each ticket's agents. A ticket workspace row SHALL be labelled "repository · ticket" with the repository name and ticket title, and SHALL show the ticket's state. There SHALL be no repository-team row or "Add repository team" action.

#### Scenario: Tree shape
- **WHEN** a project has a ticket "Fix inbound calls" in repository "sagechat-sms" with an implementer and two reviewers
- **THEN** the sidebar shows the coordinator, beneath it a row "sagechat-sms · Fix inbound calls" with the ticket's state, and beneath that the three agents

#### Scenario: New ticket picks a repository
- **WHEN** the human opens New ticket for a project
- **THEN** the dialog asks for a repository the project uses, a title and a brief, and creates the ticket under that project's coordinator

### Requirement: Role instructions match the flat hierarchy
The bundled role instructions SHALL contain no repository coordinator contract. The project coordinator contract SHALL cover planning, creating tickets per repository, assigning agents with an exact model and reason (see model-routing), starting verification with `verify_ticket`, and accepting or closing tickets. The communication, implementer and tester/reviewer contracts SHALL name the project coordinator as the agent's parent and acceptor.

#### Scenario: Bundled instructions
- **WHEN** the host builds the instructions for any role
- **THEN** none of them mentions a repository coordinator, `create_repo_coordinator` or `archive_team`
- **AND** the project coordinator's instructions name `create_ticket`, `assign_ticket`, `verify_ticket`, `accept_ticket` and `close_ticket`
