## MODIFIED Requirements

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

### Requirement: Finishing a ticket
`accept_ticket` SHALL take a ticket id and an optional one-line waiver reason. It SHALL depend on the ticket's latest verification cycle, not on the ticket's state or the kind of the last report. Every acceptance SHALL require that no cycle is running, that the latest cycle's verified commit equals the worktree's HEAD or is patch-equivalent to it, and that the ticket's ledger has no `untriaged` entries. A HEAD is patch-equivalent when the diff from its merge-base with the repository's base has the same stable patch id as the verified commit's diff from its own merge-base. When the patch id cannot be computed, only an equal commit SHALL qualify, and the refusal SHALL say why. Without a waiver, the latest cycle SHALL have passed. With a waiver, the latest cycle SHALL have ended `blocked` with no verifier whose latest result is `blocked` or still pending; a waiver after a passed cycle SHALL be refused. A waived acceptance SHALL record the reason on the ticket and record the `ticket_accepted` event with the reason and the ids of the entries still `open`; the cycle's outcome SHALL stay `blocked` and those entries SHALL stay `open`. Accepting or closing a ticket SHALL archive the ticket's agents, keep their conversations and the branch, and remove the worktree, refusing while the worktree is locked or holds uncommitted or untracked files. `close_ticket` SHALL also refuse while any of the ticket's agents has status Working, naming that agent; once an agent has reported, a turn it is still finishing only defers removal of the worktree, for accept and close alike. Neither SHALL merge, push or publish.

#### Scenario: Commits after verification
- **WHEN** the project coordinator accepts a passed ticket whose worktree HEAD adds a commit that changes the diff after the verified commit
- **THEN** the host refuses, naming the verified and current commits

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
