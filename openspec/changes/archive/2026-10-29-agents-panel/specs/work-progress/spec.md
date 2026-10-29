## REMOVED Requirements

### Requirement: Crew panel
**Reason**: Replaced by the Agents panel, which every conversation of the project can open and which also lists the project's previous agents.
**Migration**: See "Agents panel".

## ADDED Requirements

### Requirement: Agents panel
The header of every conversation of a project, its coordinator's and each of its agents', SHALL carry an Agents toggle labelled "Agents · N working", where N is the number of agents the panel lists that are in a turn, or "Agents" when none is. Activating it SHALL open the Agents panel beside the conversation, without replacing it, and activating it again SHALL close it. The panel SHALL list each counted ticket of the project, in the order the tickets were created, with its title, stage, percentage and bar. Under each ticket that is not accepted, it SHALL list that ticket's unarchived agents and the verifiers of its latest cycle, each once, including a verifier the host archived after it passed. Each agent row SHALL show its role and focus, its figure, its current step while it works, its model, its elapsed time and its bar. The elapsed time SHALL be the time since the agent's current run started while it is working, and otherwise the length of its latest run. The elapsed time SHALL refresh only while the panel is open and an agent listed under a ticket is working. Below the tickets, a "Previous agents (N)" section, folded until the human unfolds it, SHALL list every other agent the sidebar tree lists under the project's coordinator, archived or not: verifiers of earlier cycles and other archived agents, agents of accepted or closed tickets, workers created before tickets existed, and retired repository coordinators. Each previous agent's row SHALL show its role and focus and the ticket it worked on, or, without a ticket, its name, with "(retired)" after a retired repository coordinator's. Selecting any agent row SHALL select that agent's conversation, read-only when the agent is archived, and SHALL leave the panel open, still listing the whole project, with the selected agent's row marked, so the human can step through the agents. The strip SHALL follow the selected conversation. The panel SHALL stay open this way until the human closes it or opens another panel.

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
- **THEN** the ticket's card still lists the tester as "passed" at 100%, beside the working reviewer

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

### Requirement: Stuck agents
An agent SHALL be stuck when it is not archived and either its status is disconnected, or its status is ready or done and its latest turn ended in an error. A blocked agent SHALL NOT be stuck, because its coordinator answers it, and neither SHALL a paused or working agent. One rule SHALL decide this for every view that shows it. In the Agents panel, a stuck agent's row SHALL show a warning icon and its reason, "Disconnected" or "Last turn failed", in the warning color, and the card of the ticket it works on SHALL have a border in the warning color.

#### Scenario: Disconnected agent
- **WHEN** a ticket's implementer is disconnected
- **THEN** its row reads "Disconnected" in the warning color and its ticket's card has a warning border

#### Scenario: Failed last turn
- **WHEN** a reviewer's latest turn ended in an error and it is ready
- **THEN** its row reads "Last turn failed" in the warning color

#### Scenario: Blocked, paused or working
- **WHEN** an agent is blocked, paused or working, even after a failed turn
- **THEN** it is not stuck and its row and card keep their ordinary colors

### Requirement: Agents card in the coordinator's conversation
While any agent the Agents panel lists is working or stuck, the conversation of the project's coordinator SHALL show one card pinned above its composer reading "N agents working on M tickets", where M counts the tickets with an agent in a turn, followed by "· K stuck" when K agents are stuck, and an "Open Agents" action that opens the panel. With no agent working, it SHALL read "No agents working" before the stuck count. The card SHALL be in the warning color while any agent is stuck. It SHALL NOT appear in an agent's conversation or an archived coordinator's, and SHALL NOT be inserted between messages.

#### Scenario: Work under way
- **WHEN** three agents are working on two tickets and none is stuck
- **THEN** the coordinator's conversation shows "3 agents working on 2 tickets" and "Open Agents" above the composer

#### Scenario: A stuck agent
- **WHEN** one agent is working on one ticket and another agent is disconnected
- **THEN** the card reads "1 agent working on 1 ticket · 1 stuck" in the warning color

#### Scenario: Quiet project
- **WHEN** no agent is working or stuck
- **THEN** no card is shown

### Requirement: Breadcrumb back to the coordinator
The header of any conversation of a project other than its coordinator's SHALL show, above its title, a link "← <project name>" followed by the agent's ticket, such as "← Calls / sagechat-sms · Fix inbound calls". Activating the link SHALL select the project's coordinator. Such an agent's title SHALL be its role and focus when it has a ticket, and otherwise its name. The coordinator's header SHALL show the project's name and its role above its title.

#### Scenario: Back from a reviewer
- **WHEN** the human has a ticket's correctness reviewer selected and activates "← Calls" in its header
- **THEN** the project's coordinator is selected

## MODIFIED Requirements

### Requirement: Progress strip
The selected conversation SHALL show one progress strip pinned directly under its session title. The strip SHALL NOT scroll with the transcript and SHALL keep a fixed height, so progress changes never move the transcript. For a project coordinator, the strip SHALL show the project's phase label, its segmented bar, its percentage and "N of M tickets accepted". For an agent on a ticket, it SHALL show the agent's own progress, as "Step K of N" with the current step, the bar and the percentage, followed by the ticket's figure, such as "ticket 30%". K SHALL be the number of items done plus one. When every item of a working agent's plan is done, the strip SHALL read "N of N" with no step label, never "Step N+1". "Step K of N" is the label of a working agent only. A finished agent's label SHALL be "Done", or "Passed" for a verifier whose latest result is `passed`. A blocked agent's label SHALL be "Failed" for a verifier whose latest result is `failed`, and otherwise "Blocked · step K of N", or "Blocked" when its latest run had no plan. An idle agent with a checklist from its latest run SHALL read "K of N", where K is the number of items done, with no "Step" and no current item. An agent with nothing of its own to show SHALL show only its ticket's stage and figure. A session with neither a ticket nor a project of its own SHALL show its own progress alone.

#### Scenario: Coordinator selected
- **WHEN** the human selects the coordinator of a project at 56% with one of four tickets accepted
- **THEN** the strip reads "Implementing", shows the four-segment bar and "56% · 1 of 4 tickets accepted"

#### Scenario: Worker selected
- **WHEN** the human selects an implementer at 3 of 6 whose ticket is at 30%
- **THEN** the strip reads "Step 4 of 6 · Update the design README", shows a bar at 50%, and "50% · ticket 30%"

#### Scenario: Every item done
- **WHEN** the human selects a working implementer whose plan reads 6 of 6, on a ticket at 60%
- **THEN** the strip reads "6 of 6" with no step label, shows a full bar, and "100% · ticket 60%"

#### Scenario: Finished agent
- **WHEN** the human selects an implementer that reported `ready_for_testing` with its plan at 5 of 6, on a ticket at 60%
- **THEN** the strip reads "Done", not "Step 6 of 6", shows a full green bar, and "100% · ticket 60%"

#### Scenario: Passed and failed verifiers
- **WHEN** the human selects a verifier whose latest result is `passed`, and then one whose latest result is `failed` at 4 of 5
- **THEN** the first strip reads "Passed" with a full green bar, and the second reads "Failed" with a red bar at 80%

#### Scenario: Blocked agent
- **WHEN** the human selects an implementer that reported `blocked` at 3 of 7, and then one that reported `blocked` without ever writing a plan
- **THEN** the first strip reads "Blocked · step 4 of 7" with a red bar at 43%, and the second reads "Blocked" with an empty track

#### Scenario: Idle agent with a past checklist
- **WHEN** the human selects an agent whose turn ended at 4 of 6 without a report
- **THEN** the strip reads "4 of 6", with no "Step" and no current item, and shows a bar at 67%

#### Scenario: Worker with no plan
- **WHEN** the human selects a working verifier with no plan, on a ticket at 70%
- **THEN** the strip reads "Working", shows a striped bar, and "ticket 70%"

#### Scenario: Steady layout
- **WHEN** an agent's plan changes while the human reads earlier messages
- **THEN** the strip updates in place and the transcript's scroll position does not change

### Requirement: Progress in the sidebar tree
The project row in the sidebar tree SHALL show a thin bar beneath its label and the project's percentage beside it, and an archived project's row SHALL show neither bar nor percentage. Ticket rows and agent rows SHALL show neither a bar nor a progress figure, and SHALL be single-line rows whose height does not depend on progress; a ticket row with a pull request keeps its second line for the pull request. A project with nothing to show SHALL keep an empty track and no figure, so the project row's height never depends on its progress. Notes that are not progress, such as an agent's "held" or "queued input" and its questions figure, SHALL stay on the agent row.

#### Scenario: Rows under one project
- **WHEN** a project at 56% has a ticket Verifying at 70% with a finished implementer, a tester at 3 of 5 and a reviewer working without a plan
- **THEN** the project row shows "56%" and its bar, and the ticket row and the three agent rows show no figure and no bar

#### Scenario: Blocked ticket row
- **WHEN** a ticket is blocked at 26%
- **THEN** its sidebar row shows no "blocked" figure and no bar, and the strip and Agents panel still show its blocked state

#### Scenario: Project with nothing to show
- **WHEN** a project has no tickets and no progress to report
- **THEN** its row keeps an empty track and no figure, at the same height as other projects' rows

#### Scenario: Idle agent with no plan
- **WHEN** an agent is ready and its latest run had no plan
- **THEN** its row shows no figure and no track, at the same height as every other agent's and ticket's row

#### Scenario: Working agent and open ticket stay one line
- **WHEN** an agent is working at 3 of 5, or a ticket without a pull request is at 70%
- **THEN** its row has the same single-line height as an idle agent's, with no figure and no track line

### Requirement: Progress delivery without waking agents
The host SHALL keep progress current whenever a plan step, a session's status, a ticket or a verification record changes, and SHALL rebuild the same figures from stored steps and records after a restart, for every agent the board lists, including the archived verifiers of an open ticket's latest cycle. Stored steps are kept for a limited time: an agent whose latest run's steps were pruned SHALL show no checklist. Progress SHALL reach the desktop app incrementally: the host signals only when a figure actually changed, and the client then requests only the progress entries changed since its last cursor, never a full workspace snapshot. When an entry has left the board since that cursor, such as a closed ticket or an archived agent, the host SHALL answer once with the whole board, marked complete, and the client SHALL replace what it holds. A step that is not a plan step SHALL cause no progress signal. Progress changes SHALL NOT wake, notify or message any agent, and SHALL NOT delay message receipts, permission requests or failures.

#### Scenario: A plan step arrives
- **WHEN** a worker's plan goes from 2 of 5 to 3 of 5
- **THEN** the desktop receives that agent's entry, its ticket's and its project's, and no snapshot reload occurs

#### Scenario: An entry leaves the board
- **WHEN** a ticket is closed after the desktop last read the board
- **THEN** the next page is the whole board, marked complete, without that ticket, and the desktop no longer shows a figure for it

#### Scenario: Ordinary steps
- **WHEN** a worker streams narration and runs commands without changing its plan
- **THEN** no progress signal is sent

#### Scenario: Coordinator stays asleep
- **WHEN** a worker's progress changes during its turn, by a burst of `set_plan` calls or of provider plan steps
- **THEN** its coordinator is not woken and receives no message because of it, and no session's status changes

#### Scenario: Restart
- **WHEN** the host restarts while an implementer's stored plan reads 3 of 6 and its ticket is `assigned`
- **THEN** after the restart the implementer shows 3 of 6 and the ticket 30%

#### Scenario: Restart with an archived verifier
- **WHEN** a cycle ended blocked with a verifier failed at 4 of 5, the host archived it, and the host restarts
- **THEN** the Agents panel still lists that verifier as "failed" at 4 of 5

#### Scenario: Pruned run
- **WHEN** an idle agent's latest run finished so long ago that the host pruned its steps
- **THEN** the agent shows an empty track and no figure
