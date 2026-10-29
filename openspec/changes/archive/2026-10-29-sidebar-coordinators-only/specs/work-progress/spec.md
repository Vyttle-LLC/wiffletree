## REMOVED Requirements

### Requirement: Progress in the sidebar tree
**Reason**: It also governed ticket rows and agent rows, which the sidebar no longer lists.
**Migration**: See "Progress on the project row".

## ADDED Requirements

### Requirement: Progress on the project row
The project row in the sidebar tree SHALL show a thin bar beneath its label and the project's percentage beside it, and an archived project's row SHALL show neither bar nor percentage. A project with nothing to show SHALL keep an empty track and no figure, so the project row's height never depends on its progress. The tree SHALL show no progress for tickets or agents, which it does not list; the strip and the Agents panel show theirs.

#### Scenario: A project row
- **WHEN** a project at 56% has a ticket Verifying at 70% with a finished implementer, a tester at 3 of 5 and a reviewer working without a plan
- **THEN** the project row shows "56%" and its bar, and the ticket and its agents show their progress in the Agents panel

#### Scenario: Project with nothing to show
- **WHEN** a project has no tickets and no progress to report
- **THEN** its row keeps an empty track and no figure, at the same height as other projects' rows

## MODIFIED Requirements

### Requirement: Agents panel
The header of every conversation of a project, its coordinator's and each of its agents', SHALL carry an Agents toggle labelled "Agents · N working", where N is the number of agents the panel lists that are in a turn, or "Agents" when none is. Activating it SHALL open the Agents panel beside the conversation, without replacing it, and activating it again SHALL close it. The panel SHALL list each counted ticket of the project, in the order the tickets were created, with its title, stage, percentage and bar. Under each ticket that is not accepted, it SHALL list that ticket's unarchived agents, each once. Each agent row SHALL show its role and focus, its figure, its current step while it works, its model, its elapsed time and its bar. The elapsed time SHALL be the time since the agent's current run started while it is working, and otherwise the length of its latest run. The elapsed time SHALL refresh only while the panel is open and an agent listed under a ticket is working. Below the tickets, a "Previous agents (N)" section, folded until the human unfolds it, SHALL list every other agent of the project's coordinator, archived or not: archived agents, such as finished verifiers of any cycle, agents of accepted or closed tickets, workers created before tickets existed, and retired repository coordinators. An archived project has no progress, so every agent it has SHALL be listed there, and its conversations SHALL still carry the toggle. Each previous agent's row SHALL show its role and focus and the ticket it worked on, or, without a ticket, its name, with "(retired)" after a retired repository coordinator's, and its figure while the board still lists it, such as "passed". Selecting any agent row SHALL select that agent's conversation, read-only when the agent is archived, and SHALL leave the panel open, still listing the whole project, with the selected agent's row marked, so the human can step through the agents. The strip SHALL follow the selected conversation. The panel SHALL stay open this way until the human closes it or opens another panel.

#### Scenario: Open the panel
- **WHEN** the human activates "Agents · 2 working" in the coordinator's header
- **THEN** the panel opens beside the conversation and lists every counted ticket with its stage and percentage, and each open ticket's agents with current step, model and elapsed time

#### Scenario: Toggle from an agent
- **WHEN** an agent's conversation is selected and the human activates its header's Agents toggle twice
- **THEN** the panel opens listing the agent's whole project, then closes

#### Scenario: Nothing working
- **WHEN** no agent the panel lists is in a turn
- **THEN** the toggle reads "Agents"

#### Scenario: Archived passed verifier
- **WHEN** round 1 ended with the tester passed and archived, and round 2 is running with one reviewer
- **THEN** the ticket's card lists the working reviewer, and the tester is under "Previous agents" as "passed"

#### Scenario: Archived project
- **WHEN** the human shows archived sessions and selects an archived project's coordinator
- **THEN** its header offers the Agents toggle, and the panel lists every agent of the project under "Previous agents"

#### Scenario: Earlier cycle's verifiers
- **WHEN** cycle 2 is running on a ticket whose cycle 1 verifiers were archived
- **THEN** the card lists cycle 2's verifiers, and cycle 1's are under "Previous agents"

#### Scenario: Accepted ticket
- **WHEN** a ticket is accepted
- **THEN** its card shows "Accepted · 100%" and no agent rows, and its agents are under "Previous agents"

#### Scenario: Agents from before tickets
- **WHEN** a project has a worker created before tickets existed and a retired repository coordinator
- **THEN** "Previous agents (2)" lists the worker by name and the coordinator as "<name> (retired)", and selecting either opens its conversation

#### Scenario: Step through the agents
- **WHEN** the panel is open and the human selects the implementer's row, then a reviewer's row
- **THEN** each selection shows that agent's conversation and strip, the panel stays open listing the whole project, and the selected agent's row is marked

#### Scenario: Leave the panel
- **WHEN** an agent selected from the panel is shown and the human closes the panel
- **THEN** the panel closes, and that agent's header still offers the Agents toggle

#### Scenario: Idle panel
- **WHEN** the panel is open and no agent listed under a ticket is working
- **THEN** no elapsed-time timer runs
