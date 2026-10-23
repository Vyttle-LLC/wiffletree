## ADDED Requirements

### Requirement: A ticket may start on another branch of origin
`create_ticket` SHALL take an optional `base`: the name of a branch on the repository's `origin`, such as `Parental-controls`. The host SHALL fetch that branch into `origin/<base>` before creating anything, and SHALL refuse a name Git does not accept as a branch name and a branch the fetch does not find on `origin`, including one only the local clone has, creating nothing. `base` and `stack_on` SHALL NOT be given together. A ticket created with `base` SHALL record it, `workspace_context` SHALL show it on the ticket's full and short entries, its worktree and branch SHALL start at `origin/<base>`, and it SHALL be measured against `origin/<base>`: the reviewers' base and diff, the size a publish reports, and whether a later commit has the verified commit's patch. Repeating `create_ticket` with the same coordinator, repository and title SHALL return the existing ticket only when the `base` is the same, and SHALL otherwise be refused with "Ticket title already exists on another base". A ticket without `base` SHALL be created and measured as before.

#### Scenario: Started on a branch of origin
- **WHEN** the coordinator creates a ticket with `base` `release`, a branch on `origin` one commit past `main`
- **THEN** its worktree starts at that commit, its `base` is `release`, and `workspace_context` shows it

#### Scenario: Reviewers see only the ticket's changes
- **WHEN** a ticket created on `release` has one commit of its own
- **THEN** its diff's base is `origin/release` and the diff is that one commit

#### Scenario: A branch not on origin
- **WHEN** `base` names a branch only the local clone has, or a name such as `bad..name`
- **THEN** creation is refused, saying the branch is not on origin or is not a branch name, and no ticket is created

#### Scenario: Both a parent and a base
- **WHEN** `create_ticket` is given both `stack_on` and `base`
- **THEN** it is refused and nothing is created

#### Scenario: The same title on another base
- **WHEN** a ticket "On release" exists on `release`, and `create_ticket` is repeated with that title and `base` `main`, or with no `base`
- **THEN** it is refused with "Ticket title already exists on another base"
- **AND** repeating it with `base` `release` returns the existing ticket

### Requirement: Open-branch warnings compare only tickets on the same base
The open-branch check SHALL group a repository's open tickets by their base, `origin/<base>` for a ticket with a `base` and otherwise the repository's base, and SHALL fetch each base and compare each ticket only with that base and with the other tickets on it. Tickets on the repository's base SHALL be checked as before.

#### Scenario: A ticket on another base
- **WHEN** a repository has an open ticket on its base and one created on `release`
- **THEN** the ticket on `release` is checked against `origin/release` in a check of its own

## MODIFIED Requirements

### Requirement: A ticket is a workspace
A ticket SHALL belong to one project coordinator and one repository that the project uses, and SHALL own exactly one worktree and one branch for its whole life. `create_ticket` SHALL take `title`, `brief`, `repository_id`, an optional `stack_on` and an optional `base`, create the worktree and branch from the repository's base, from the `stack_on` ticket's branch tip, or from `origin/<base>` when given, and store `repository_id` on the ticket. Every agent assigned to the ticket SHALL work in that worktree, and no other operation SHALL create a worktree for it except re-creating the same path from the same branch on restore. Repeating `create_ticket` with the same coordinator, repository and title SHALL return the existing ticket.

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

### Requirement: A ticket may stack on another ticket in its repository
`create_ticket` with `stack_on` SHALL accept only a ticket of the same project and repository that is not closed, and SHALL otherwise refuse, naming the parent and why, and create nothing. The ticket SHALL record the parent as `stacked_on`, and `workspace_context` SHALL show it on the ticket's full and short entries. A stacked ticket SHALL be measured against its parent's branch: the reviewers' base and diff, the size a publish reports, and whether a later commit has the verified commit's patch. A later commit SHALL NOT count as having that patch because it has it against the repository's base. A stacked ticket SHALL record its parent's `base`, so its base once the parent has merged is the parent's. Without `stack_on`, creation and measurement SHALL be unchanged.

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

#### Scenario: Stacked on a ticket with a base
- **WHEN** the coordinator creates "Child" with `stack_on` naming a ticket created with `base` `release`
- **THEN** the child's `base` is `release`
