## MODIFIED Requirements

### Requirement: Role instructions match the flat hierarchy
The bundled role instructions SHALL contain no repository coordinator contract. The project coordinator contract SHALL cover planning, creating tickets per repository, assigning agents with an exact model and reason (see model-routing), starting verification with `verify_ticket`, triaging every untriaged finding once with `triage_findings`, leaving `noted` findings undecided unless one should be fixed, pulling such a finding into a fix with `triage_findings` `fix_now` and sending it to the implementer, accepting with or without a waiver, and closing tickets. It SHALL state that non-blocking and pre-existing findings are `noted` and need no decision before accepting or publishing. It SHALL state the stop rule: a round with only non-blocking findings ends the loop, and so do non-blocking findings left after two or three rounds; they stay `noted`, or become follow-up or won't-fix entries when the coordinator decides so, and the coordinator never marks them `fix_now` to chase them and never starts a fresh cycle to chase them. It SHALL tell the coordinator that a ticket shipping as a PR lists its follow-up entries in the PR description. It SHALL also tell the coordinator to ask the human when a waiver would cover a product, security or data-loss decision. The implementer contract SHALL tell the implementer to fix each routed finding with the smallest change, to make only changes that are requested or clearly necessary, and to tell the coordinator in one sentence with evidence, instead of implementing it, when a finding is wrong or out of scope. The tester and reviewer contract SHALL require every finding to be reported with its severity, `path:line` location, trigger and evidence, without patches. It SHALL define blocking as breaking an acceptance criterion, breaking existing behavior or tests, or having a concrete trigger reachable in this change. A race SHALL block only with a failing test or a step-by-step trace citing `file:line`, and bugs that predate the change SHALL be `pre_existing`. The communication, implementer and tester/reviewer contracts SHALL name the project coordinator as the agent's parent and acceptor.

#### Scenario: Bundled instructions
- **WHEN** the host builds the instructions for any role
- **THEN** none of them mentions a repository coordinator, `create_repo_coordinator` or `archive_team`
- **AND** the project coordinator's instructions name `create_ticket`, `assign_ticket`, `verify_ticket`, `triage_findings`, `accept_ticket` and `close_ticket`, and state the stop rule

#### Scenario: Noted findings in the coordinator's instructions
- **WHEN** the host builds the project coordinator's instructions
- **THEN** they say that non-blocking and pre-existing findings are `noted` and need no decision before accepting or publishing, that a noted finding is pulled into a fix with `triage_findings` `fix_now` and sent to the implementer, and that a PR's description lists the ticket's follow-up entries
- **AND** they no longer tell the coordinator to triage non-blocking findings as follow-up or won't-fix before accepting

#### Scenario: Implementer may object
- **WHEN** the host builds the implementer's instructions
- **THEN** they tell it to raise a wrong or out-of-scope finding with the coordinator in one sentence with evidence, and no longer say "Do not argue"

#### Scenario: Verifier findings format
- **WHEN** the host builds the tester and reviewer instructions
- **THEN** they require severity, `path:line`, trigger and evidence for every finding, forbid patches, and define blocking
