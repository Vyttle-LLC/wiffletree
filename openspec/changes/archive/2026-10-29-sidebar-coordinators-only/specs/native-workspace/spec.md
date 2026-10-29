## ADDED Requirements

### Requirement: The sidebar lists projects only
The sidebar tree SHALL list group rows and project rows only, one project row for each coordinator. It SHALL NOT list tickets or agents, archived, legacy or retired ones included; the Agents panel lists them (see work-progress). A project row SHALL have no chevron and SHALL NOT collapse. Its last line SHALL read "N agents working", counting the agents the Agents panel lists for the project that are in a turn, and SHALL be left out when none is. When the stuck rule (see work-progress) flags any of the project's unarchived agents, that line SHALL also show a yellow pill reading "1 agent stuck" or "N agents stuck", and the row's tooltip SHALL say the same. The row SHALL keep its glyph, figure, progress track and needs-you badge. While the coordinator's or any of the project's agents' conversation is selected, the project row SHALL show as selected.

#### Scenario: Only projects
- **WHEN** a project has an open ticket with an implementer, an archived reviewer, a closed ticket and a worker from before tickets existed, and archived sessions are shown
- **THEN** the sidebar shows one row for the project and none for its tickets or agents

#### Scenario: Agents working
- **WHEN** two of a project's agents are in a turn and none is stuck
- **THEN** its row's last line reads "2 agents working" with no pill

#### Scenario: A stuck agent
- **WHEN** one of a project's agents is disconnected and another is in a turn
- **THEN** its row's last line reads "1 agent working" beside a yellow "1 agent stuck" pill, and its tooltip says "1 agent stuck"

#### Scenario: A blocked agent is not stuck
- **WHEN** a project's only agent is blocked after a failed turn
- **THEN** its row shows no agents line and no pill

#### Scenario: An agent is open
- **WHEN** the human selects one of a project's agents from the Agents panel
- **THEN** that project's row shows as selected

## MODIFIED Requirements

### Requirement: Questions waiting beside an agent's status
An agent's status badge SHALL name the questions it still waits on after its status, as "Working · 1 question waiting" or "Blocked · 2 questions waiting". The badge text SHALL use the status's own color, never yellow, because a worker's question goes to its coordinator and yellow is reserved for what needs the human. The question's text SHALL NOT be repeated beside the status. A session with no question waiting SHALL show none.

#### Scenario: Working with a question
- **WHEN** an implementer reported `blocked` once and its turn is still running
- **THEN** its badge reads "Working · 1 question waiting"

#### Scenario: Turn ended unanswered
- **WHEN** that turn ends and the coordinator has not answered
- **THEN** its badge reads "Blocked · 1 question waiting"

#### Scenario: Nothing waiting
- **WHEN** a session has no unanswered `blocked` report
- **THEN** its badge shows no question text

### Requirement: A group row collapses and rolls up what is inside
A group row SHALL show a chevron, a folder icon, the group's label and how many projects it holds, and activating it SHALL collapse or expand the group. A collapsed group SHALL show the most urgent state among its shown members: the number of items that need the human when there are any, otherwise the number of its members' stuck agents as the stuck pill when there are any, otherwise blocked when a member's row would show blocked, otherwise working when a member's row would show working, and otherwise nothing.

#### Scenario: Needs-you outranks blocked and working
- **WHEN** a collapsed group holds one project with one item needing the human, one blocked project and one working project
- **THEN** the group row shows the needs-you count "1"

#### Scenario: Stuck agents outrank blocked and working
- **WHEN** a collapsed group holds a project with one stuck agent, a blocked project and a working project, and nothing needs the human
- **THEN** the group row shows the "1 agent stuck" pill

#### Scenario: Blocked outranks working
- **WHEN** a collapsed group holds a blocked project and a working project, nothing needs the human and no agent is stuck
- **THEN** the group row shows the blocked mark

#### Scenario: A quiet group
- **WHEN** every project in a collapsed group is ready, done or paused
- **THEN** the group row shows its count and no state

### Requirement: Projects are reordered by dragging
A person SHALL be able to drag project rows to reorder them, and each drop SHALL be sent to the host as one move, with the list redrawn from the order the host returns. While grouping is off, any project row SHALL be droppable at any place in the flat list. While grouping is on, a project inside a group SHALL be droppable only within that group, and a group row or an ungrouped project row SHALL be droppable only among the other groups and ungrouped projects; dragging a group SHALL move all its shown members as one block, and reordering inside a group SHALL NOT move the group. While a row is dragged, an insertion line SHALL show where it would land and the dragged row SHALL be dimmed. While a row is dragged and the pointer is near the top or bottom edge of the list, the list SHALL scroll in that direction, also while the pointer rests there, until it reaches its end; it SHALL stop scrolling when the pointer leaves that edge or the list, or the drag ends, and the insertion line SHALL keep to the row under the pointer as the list scrolls. A project row whose name is being edited SHALL NOT start a drag. A drag that left its row and is released back on it SHALL change nothing. Dragging SHALL NOT change what a click, a double-click or the actions menu does, including when the pointer moves a little between press and release without leaving the row.

#### Scenario: A project moves within the flat list
- **WHEN** grouping is off, the list is A, B, C and C is dropped above A
- **THEN** the host's order becomes C, A, B and the list shows it

#### Scenario: A project moves within its group
- **WHEN** the group "wiffletree" holds "Reorder", "Owners", "Compact" and "Compact" is dropped above "Reorder"
- **THEN** the group holds "Compact", "Reorder", "Owners" and stays where it was among the other rows

#### Scenario: The top project of a group moves down
- **WHEN** the group "wiffletree" sits above "ci-runners: Autoscaling" and its topmost project is dropped at the bottom of the group
- **THEN** the group still sits above "ci-runners: Autoscaling"

#### Scenario: A whole group moves
- **WHEN** the group "watch", holding two projects, is dropped above the group "wiffletree"
- **THEN** both of its projects sit directly above the topmost "wiffletree" project in the host's order, in the order they had

#### Scenario: A grouped project cannot leave its group
- **WHEN** a project inside the group "wiffletree" is dragged over a row outside that group
- **THEN** no insertion line appears there and dropping it there moves nothing

#### Scenario: A row is dropped at a place that was off-screen
- **WHEN** the list is taller than the sidebar and a row is dragged to its bottom edge and held there
- **THEN** the list scrolls down to its end, and the row can be dropped beside a row that was not visible when the drag began

#### Scenario: A drag returned to its row is cancelled
- **WHEN** a group row is dragged past other rows, brought back and released on itself
- **THEN** nothing moves and the group stays expanded or collapsed as it was

#### Scenario: A click still selects
- **WHEN** a project row is clicked without moving the pointer
- **THEN** its conversation opens and nothing is moved

#### Scenario: A click that moves is still a click
- **WHEN** the pointer moves a few pixels between press and release without leaving the row
- **THEN** a project row is selected, a group row is collapsed or expanded, and the second click of a double-click opens rename, as each does without moving
