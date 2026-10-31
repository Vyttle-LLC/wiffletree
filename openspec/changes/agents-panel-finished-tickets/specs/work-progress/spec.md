## MODIFIED Requirements

### Requirement: Agents panel
The header of every conversation of a project, its coordinator's and each of its agents', SHALL carry an Agents toggle labelled "Agents · N working", where N is the number of agents the panel lists that are in a turn, or "Agents" when none is. Activating it SHALL open the Agents panel beside the conversation, without replacing it, and activating it again SHALL close it. The panel's heading SHALL show the open and finished tickets as drawn, such as "9 open · 32 finished": every card counts as open and every line of the fold as finished, and with neither it shows no figure. The panel SHALL draw a card for each counted ticket that is not accepted, in the order the tickets were created, with its title, stage, percentage and bar, followed by a card for each accepted or closed ticket that has an unarchived agent, such as one restored from the archive. A finished ticket's card SHALL show its stage and percentage, or "Closed" with no bar for a closed ticket, and its PR only as its outcome. Under each card, it SHALL list that ticket's unarchived agents, each once. Each agent row SHALL show its role and focus, its figure, its current step while it works, its model, its elapsed time and its bar. An agent of an open ticket whose status is ready or done, that is not stuck, has no compaction running and has no recorded result in the ticket's latest cycle SHALL instead show one quiet line with its role and focus and what it waits for, with no bar or model, and SHALL stay selectable: "Waiting for review" while the ticket is ready for testing or verifying, "Waiting on PR #N" while its PR is open, and otherwise "Waiting on coordinator"; a verifier still pending in that cycle SHALL read "Review pending". It SHALL return to a full row when it starts a turn. The elapsed time SHALL be the time since the agent's current run started while it is working, and otherwise the length of its latest run. The elapsed time SHALL refresh only while the panel is open and an agent listed under a ticket is working. Below the cards, a "Finished tickets (N)" section, folded until the human unfolds it, SHALL list every other accepted or closed ticket of the project's coordinator, whichever half of the snapshot holds it, one line each: its title and its PR's outcome, "#1 merged", "#2 closed" or "#3 open", or "Closed, no PR" or "Accepted, no PR". A line SHALL show no bar, checks or conflict, and a PR still open SHALL read as quietly as a closed one, never in the error color. A ticket accepted with a waiver SHALL keep its waiver note directly under its line (see ticket-workspaces). Below it, a "Previous agents (N)" section, folded until the human unfolds it, SHALL list every other agent of the project's coordinator, archived or not: archived agents, such as finished verifiers of any cycle, agents of accepted or closed tickets, workers created before tickets existed, and retired repository coordinators. An archived project has no progress, so every agent it has SHALL be listed there, and its conversations SHALL still carry the toggle. Each previous agent's row SHALL show its role and focus and the ticket it worked on, or, without a ticket, its name, with "(retired)" after a retired repository coordinator's, and its figure while the board still lists it, such as "passed". Selecting any agent row SHALL select that agent's conversation, read-only when the agent is archived, and SHALL leave the panel open, still listing the whole project, with the selected agent's row marked, so the human can step through the agents. The strip SHALL follow the selected conversation. The panel SHALL stay open this way until the human closes it or opens another panel.

#### Scenario: Open the panel
- **WHEN** the human activates "Agents · 2 working" in the coordinator's header
- **THEN** the panel opens beside the conversation and lists every open ticket with its stage and percentage, and each open ticket's agents with current step, model and elapsed time

#### Scenario: Toggle from an agent
- **WHEN** an agent's conversation is selected and the human activates its header's Agents toggle twice
- **THEN** the panel opens listing the agent's whole project, then closes

#### Scenario: Nothing working
- **WHEN** no agent the panel lists is in a turn
- **THEN** the toggle reads "Agents"

#### Scenario: Archived passed verifier
- **WHEN** the tester passed and was archived, and the reviewer of the same round is working
- **THEN** the ticket's card lists the working reviewer, and the tester is under "Previous agents" as "passed"

#### Scenario: Archived project
- **WHEN** the human shows archived sessions and selects an archived project's coordinator
- **THEN** its header offers the Agents toggle, and the panel lists every agent of the project under "Previous agents"

#### Scenario: Earlier cycle's verifiers
- **WHEN** cycle 2 is running on a ticket whose cycle 1 verifiers were archived
- **THEN** the card lists cycle 2's verifiers, and cycle 1's are under "Previous agents"

#### Scenario: Accepted ticket
- **WHEN** a ticket is accepted
- **THEN** it has no card, "Finished tickets (N)" lists it on one line, and its agents are under "Previous agents"

#### Scenario: Closed ticket
- **WHEN** a ticket with no PR is closed, and the archive half of the snapshot holds it
- **THEN** "Finished tickets (N)" lists it as "Closed, no PR", and the project's bar and percentage do not change

#### Scenario: A finished ticket's PR still open
- **WHEN** an accepted ticket's PR is recorded as open with a conflict
- **THEN** its line reads "#N open" in the muted color, with no checks or conflict

#### Scenario: A finished ticket with an agent
- **WHEN** an agent of a closed ticket is restored from the archive
- **THEN** the ticket is a card reading "Closed" with no bar, listing that agent, and is not in "Finished tickets (N)"

#### Scenario: Every ticket finished
- **WHEN** every ticket of a project is finished and none has an unarchived agent
- **THEN** the panel shows no card and no empty state, its heading reads "0 open · N finished", and the fold lists them

#### Scenario: An implementer waiting
- **WHEN** an implementer has reported `ready_for_testing` and a cycle is running on its ticket
- **THEN** it shows one quiet line reading "Waiting for review", and selecting it opens its conversation

#### Scenario: An implementer waiting on a PR
- **WHEN** an idle implementer's ticket passed and its PR 21 is open
- **THEN** its line reads "Waiting on PR #21"

#### Scenario: A stuck agent is never quiet
- **WHEN** an idle implementer's latest turn ended in an error
- **THEN** it keeps its full row with "Last turn failed"

#### Scenario: A waiting agent starts a turn
- **WHEN** an agent on a quiet line starts a turn
- **THEN** it returns to a full row

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
