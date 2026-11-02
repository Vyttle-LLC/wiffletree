## MODIFIED Requirements

### Requirement: A ticket is a workspace
A ticket SHALL belong to one project coordinator and one repository that the project uses, and SHALL own exactly one worktree and one branch for its whole life. `create_ticket` SHALL take `title`, `brief`, `repository_id`, an optional `stack_on` and an optional `base`, create the worktree and branch from the repository's base, from the `stack_on` ticket's branch tip, or from `origin/<base>` when given, and store `repository_id` on the ticket. Every agent assigned to the ticket SHALL work in that worktree, and no other operation SHALL create a worktree for it except re-creating the same path from the same branch on restore. Repeating `create_ticket` with the same coordinator, repository and title SHALL return the existing ticket.

#### Scenario: One worktree per ticket
- **WHEN** a single-repository, single-ticket task (SH-1171-sized) runs from `create_ticket` through implementation, verification by three verifiers and `accept_ticket`
- **THEN** exactly one worktree was registered for the ticket in its repository (`git worktree list` shows the ticket's `worktree` path and no other path on its `branch`)
- **AND** the implementer's and every verifier's runtime `workdir` equals the ticket's `worktree`

#### Scenario: Repository outside the project
- **WHEN** the coordinator of project Billing, in the Vyttle Space, calls `create_ticket` with `wiffletree`, which the project does not use
- **THEN** the host rejects it with "Project Billing does not use repository wiffletree: Vyttle does not list it and it was not added to this project. A coordinator asks for it with request_repository, and the human answers in the Inbox.", and creates no worktree or branch

#### Scenario: Repeated creation is idempotent
- **WHEN** `create_ticket` is called twice with the same title, brief and repository
- **THEN** the same ticket is returned and one worktree exists

#### Scenario: Several tickets share a repository
- **WHEN** the project coordinator creates two tickets in the same repository
- **THEN** each ticket has its own worktree and branch

#### Scenario: A reviewer on a ticket whose repository left the project's Space
- **WHEN** a repository leaves the Space, or the project moves, while an open ticket works there, and a verifier starts on that ticket
- **THEN** it starts in the ticket's worktree
