## MODIFIED Requirements

### Requirement: A ticket is a workspace
A ticket SHALL belong to one project coordinator and one repository that the project uses, and SHALL own exactly one worktree and one branch for its whole life. `create_ticket` SHALL take `title`, `brief`, `repository_id` and an optional `stack_on`, create the worktree and branch from the repository's base, or from the `stack_on` ticket's branch tip when given, and store `repository_id` on the ticket. Every agent assigned to the ticket SHALL work in that worktree, and no other operation SHALL create a worktree for it except re-creating the same path from the same branch on restore. Repeating `create_ticket` with the same coordinator, repository and title SHALL return the existing ticket.

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

## ADDED Requirements

### Requirement: A ticket may stack on another ticket in its repository
`create_ticket` with `stack_on` SHALL accept only a ticket of the same project and repository that is not closed, and SHALL otherwise refuse, naming the parent and why, and create nothing. The ticket SHALL record the parent as `stacked_on`, and `workspace_context` SHALL show it on the ticket's full and short entries. A stacked ticket SHALL be measured against its parent's branch: the reviewers' base and diff, the size a publish reports, and whether a later commit has the verified commit's patch. A later commit SHALL also count as having that patch when it has it against the repository's base. Without `stack_on`, creation and measurement SHALL be unchanged.

#### Scenario: Stacked on a reviewed ticket
- **WHEN** the coordinator creates "Child" with `stack_on` naming a ticket whose branch has two commits
- **THEN** the child's worktree starts at the parent's branch tip and its `stacked_on` is the parent's id

#### Scenario: A parent elsewhere
- **WHEN** `stack_on` names a ticket in another repository, in another project, a closed ticket or no ticket
- **THEN** creation is refused with the reason and no worktree or branch is created

#### Scenario: Reviewers see only the child's changes
- **WHEN** a verifier starts on a stacked ticket with one commit of its own
- **THEN** its base is the parent's branch tip and its diff is that one commit

#### Scenario: The parent gains a fix
- **WHEN** the parent gains a commit and the child is rebased onto it
- **THEN** the child's HEAD still has the verified commit's patch, and accepting or publishing it needs no new review
