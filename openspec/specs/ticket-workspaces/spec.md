# ticket-workspaces Specification

## Purpose
Coordinate a project through one project coordinator whose tickets each own a repository worktree, branch and agents.

## Requirements

### Requirement: Two-level coordination
A project SHALL have exactly one coordinator session, its project coordinator (role `project_orchestrator`). The project coordinator SHALL create tickets, assign agents to them, triage their findings, and accept or close them directly. The host SHALL NOT create sessions with role `task_orchestrator`, and the agent tool list SHALL NOT offer `create_repo_coordinator` or `archive_team`.

#### Scenario: Repository coordinator tools are gone
- **WHEN** any managed session lists its workspace tools
- **THEN** the list contains `create_ticket`, `assign_ticket`, `verify_ticket`, `triage_findings`, `accept_ticket` and `close_ticket`
- **AND** it contains neither `create_repo_coordinator` nor `archive_team`

#### Scenario: Only the project coordinator manages tickets
- **WHEN** an implementer, tester or reviewer calls `create_ticket`, `assign_ticket`, `verify_ticket`, `triage_findings`, `accept_ticket` or `close_ticket`
- **THEN** the host rejects the call without changing any ticket or session

#### Scenario: Legacy role cannot be created
- **WHEN** a client sends `CreateSession` with role `task_orchestrator`
- **THEN** the host rejects it and no session is stored

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
`accept_ticket` SHALL take a ticket id and an optional one-line waiver reason. It SHALL depend on the ticket's latest verification cycle, not on the ticket's state or the kind of the last report. Every acceptance SHALL require that no cycle is running, that the latest cycle's verified commit equals the worktree's HEAD or is patch-equivalent to it, or that the waiver covers the commits past it as below, and that the ticket's ledger has no `untriaged` entries. A HEAD is patch-equivalent when the diff from its merge-base with the repository's base has the same stable patch id as the verified commit's diff from its own merge-base. When the patch id cannot be computed, only an equal commit SHALL qualify, and the refusal SHALL say why. No acceptance, with or without a waiver, SHALL be given while the ticket records a head that is not reviewed, as after the PR keeper adopted commits Wiffletree did not make (see pr-keeper), whatever the HEAD's patch id; the refusal SHALL say that the branch has commits no review has checked. Without a waiver, the latest cycle SHALL have passed. With a waiver, the latest cycle SHALL have ended `blocked` with no verifier whose latest result is `blocked` or still pending; a waiver after a passed cycle SHALL be refused, unless it covers commits no review checked. Once the ticket has started as many full reviews as the cycle cap allows, a waiver MAY cover the commits from the verified commit to HEAD when it names every one of them by a short SHA of at least 7 characters; the refusal SHALL list the short SHAs of any it does not name. Below the cap, a waiver SHALL NOT cover such commits, and they need a review. A waived acceptance SHALL record the commits it covered with the reason. A waived acceptance SHALL record the reason on the ticket and record the `ticket_accepted` event with the reason and the ids of the entries still `open`; the cycle's outcome SHALL stay `blocked` and those entries SHALL stay `open`. Accepting or closing a ticket SHALL archive the ticket's agents, keep their conversations and the branch, and remove the worktree, refusing while the worktree is locked or holds uncommitted or untracked files. `close_ticket` SHALL also refuse while any of the ticket's agents has status Working, naming that agent; once an agent has reported, a turn it is still finishing only defers removal of the worktree, for accept and close alike. Neither SHALL merge, push or publish.

#### Scenario: Commits after verification
- **WHEN** the project coordinator accepts a passed ticket whose worktree HEAD adds a commit that changes the diff after the verified commit
- **THEN** the host refuses, naming the verified and current commits

#### Scenario: Unreviewed commits at the cap
- **WHEN** a ticket has used every full review the cycle cap allows, its latest cycle passed, two commits follow the verified one, and the coordinator accepts with a waiver naming both by short SHA
- **THEN** the ticket is `accepted` and records the waiver with both commits

#### Scenario: A commit the waiver does not name
- **WHEN** at the cap the waiver names only the first of those commits
- **THEN** `accept_ticket` refuses, naming the second commit's short SHA, and the ticket stays open

#### Scenario: Below the cap
- **WHEN** the ticket may still start a full review and the coordinator accepts with a waiver naming every commit after the verified one
- **THEN** `accept_ticket` refuses, and the commits need a review

#### Scenario: Squashed after verification
- **WHEN** the implementer squashes the verified commits into one commit with the same diff and the coordinator accepts the ticket
- **THEN** the ticket is `accepted` without a new cycle

#### Scenario: Report after a passed cycle
- **WHEN** a cycle passed at HEAD, the implementer then reports `completed` after opening a PR, and the coordinator accepts the ticket after the PR merges
- **THEN** the ticket is `accepted`

#### Scenario: Untriaged findings block acceptance
- **WHEN** the latest cycle passed and the ledger still has an `untriaged` entry
- **THEN** `accept_ticket` refuses, naming the entry

#### Scenario: Accept with a waiver
- **WHEN** cycle 2 ended `blocked` at the round cap with F3 `open`, no verifier reported `blocked`, the ledger has no `untriaged` entry, and the coordinator accepts with the waiver "F3 needs a base ref the host always fetches"
- **THEN** the ticket is `accepted` with that waiver, cycle 2's outcome is still `blocked`, F3 is still `open`, and the `ticket_accepted` event names the waiver and F3

#### Scenario: Waiver refused
- **WHEN** the latest cycle ended `blocked` because a verifier reported `blocked`, or the latest cycle passed, and the coordinator accepts with a waiver
- **THEN** the host refuses and the ticket is unchanged

#### Scenario: Accepting archives the workspace
- **WHEN** the project coordinator accepts a passed ticket with a clean worktree and no agent in its turn
- **THEN** the ticket is `accepted`, its agents are archived with their conversations readable, its branch exists and its worktree path is gone

#### Scenario: Adopted commits with the same patch id
- **WHEN** the PR keeper adopted a commit that only re-indents a line, so the worktree's HEAD has the verified commit's stable patch id, and the coordinator accepts the ticket before any later review
- **THEN** the host refuses, saying the branch has commits no review has checked

### Requirement: Ticket workspaces in the sidebar
The sidebar SHALL show each project as its coordinator, then its ticket workspaces, then each ticket's agents. A ticket workspace row SHALL be labelled "repository · ticket" with the repository name and ticket title, and SHALL show the ticket's state. A ticket accepted with a waiver SHALL show a distinct waived glyph, never the glyph for passed or accepted, and its tooltip SHALL say it was accepted with a waiver and how many entries are still `open`. Accepted tickets, waived or not, SHALL stay hidden with their archived agents unless archived sessions are shown. There SHALL be no repository-team row or "Add repository team" action.

#### Scenario: Tree shape
- **WHEN** a project has a ticket "Fix inbound calls" in repository "sagechat-sms" with an implementer and two reviewers
- **THEN** the sidebar shows the coordinator, beneath it a row "sagechat-sms · Fix inbound calls" with the ticket's state, and beneath that the three agents

#### Scenario: Waived ticket
- **WHEN** archived sessions are shown and a project has one passed ticket, one blocked ticket and one ticket accepted with a waiver
- **THEN** the waived ticket's row shows the waived glyph, distinct from the passed and blocked rows, and its tooltip reads "Accepted with waiver" with its number of open entries

#### Scenario: New ticket picks a repository
- **WHEN** the human opens New ticket for a project
- **THEN** the dialog asks for a repository the project uses, a title and a brief, and creates the ticket under that project's coordinator

### Requirement: Role instructions match the flat hierarchy
The bundled role instructions SHALL contain no repository coordinator contract. The project coordinator contract SHALL cover planning, creating tickets per repository, assigning agents with an exact model and reason (see model-routing), starting verification with `verify_ticket`, triaging every untriaged finding once with `triage_findings`, accepting with or without a waiver, and closing tickets. It SHALL state the stop rule: after two or three rounds, remaining non-blocking findings become follow-up or won't-fix entries, and the coordinator never starts a fresh cycle to chase them. It SHALL also tell the coordinator to ask the human when a waiver would cover a product, security or data-loss decision. The implementer contract SHALL tell the implementer to fix each routed finding with the smallest change, to make only changes that are requested or clearly necessary, and to tell the coordinator in one sentence with evidence, instead of implementing it, when a finding is wrong or out of scope. The tester and reviewer contract SHALL require every finding to be reported with its severity, `path:line` location, trigger and evidence, without patches. It SHALL define blocking as breaking an acceptance criterion, breaking existing behavior or tests, or having a concrete trigger reachable in this change. A race SHALL block only with a failing test or a step-by-step trace citing `file:line`, and bugs that predate the change SHALL be `pre_existing`. The communication, implementer and tester/reviewer contracts SHALL name the project coordinator as the agent's parent and acceptor.

#### Scenario: Bundled instructions
- **WHEN** the host builds the instructions for any role
- **THEN** none of them mentions a repository coordinator, `create_repo_coordinator` or `archive_team`
- **AND** the project coordinator's instructions name `create_ticket`, `assign_ticket`, `verify_ticket`, `triage_findings`, `accept_ticket` and `close_ticket`, and state the stop rule

#### Scenario: Implementer may object
- **WHEN** the host builds the implementer's instructions
- **THEN** they tell it to raise a wrong or out-of-scope finding with the coordinator in one sentence with evidence, and no longer say "Do not argue"

#### Scenario: Verifier findings format
- **WHEN** the host builds the tester and reviewer instructions
- **THEN** they require severity, `path:line`, trigger and evidence for every finding, forbid patches, and define blocking

### Requirement: Removing a ticket's worktree is whole whatever its size
Removing a ticket's worktree, on accept, close, archive, a pending removal or an approved leftover removal, SHALL either remove it completely or leave it whole at its path with all its files, whatever the size of its folder, including ignored build output. The host SHALL rename the folder aside on the same filesystem, have Git forget the worktree within the usual Git time limit, and then delete the renamed folder without a time limit and without holding up other host work. If Git fails, the host SHALL rename the folder back and run the same cleanliness check removal began with; if the worktree is as clean as it was checked to be, the removal SHALL fail as before with the worktree whole, and if it is not or the check fails, the host SHALL complete the removal, which loses nothing because the worktree had no uncommitted or untracked files. A worktree SHALL never be left broken at its path. At startup the host SHALL delete any folder a stop left renamed aside and have Git forget registrations a cut-off removal left. The ticket's branch SHALL never be deleted.

#### Scenario: A large worktree is removed whole
- **WHEN** a ticket whose worktree holds thousands of ignored build files is closed
- **THEN** the ticket is closed, its worktree path is gone, Git no longer lists the worktree, the renamed folder is deleted and the branch remains

#### Scenario: Git fails to forget the worktree
- **WHEN** Git fails while forgetting the worktree of a ticket being closed
- **THEN** the close fails, the ticket stays open, and the worktree is registered at its path with all its files and no unsaved changes

#### Scenario: Git fails after forgetting the worktree
- **WHEN** Git forgets the worktree of a ticket being closed and then fails, for example by reaching its time limit
- **THEN** the removal completes: the ticket is closed, its worktree path is gone and the renamed folder is deleted

#### Scenario: Git is cut off partway through forgetting the worktree
- **WHEN** Git, removing the worktree of a ticket being closed, leaves its registration incomplete and then fails
- **THEN** the removal completes: the ticket is closed, its worktree path is gone, its branch remains, and the next start has Git forget the registration

#### Scenario: A stop during deletion
- **WHEN** the host starts and a ticket's renamed-aside folder is still present
- **THEN** that folder is deleted and the ticket's worktree is untouched

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
