## MODIFIED Requirements

### Requirement: The sidebar lists projects only
Above the PROJECTS header, the sidebar SHALL show two pinned rows: the assistant, by the name the user gave it, and Boards. The assistant row SHALL show a working mark while any thread or job run is in a turn, and a count of job items needing attention (see scheduled-jobs); selecting it SHALL open the assistant with its thread tabs. Selecting Boards SHALL open the Boards library. The sidebar tree below SHALL list group rows and project rows only, one project row for each coordinator. It SHALL NOT list tickets, agents or threads, archived, legacy or retired ones included; the Agents panel lists agents (see work-progress) and the assistant lists threads. A project row SHALL have no chevron and SHALL NOT collapse. Its last line SHALL read "N agents working", counting the agents the Agents panel lists for the project that are in a turn, and SHALL be left out when none is. When the stuck rule (see work-progress) flags any of the project's unarchived agents, that line SHALL also show a yellow pill reading "1 agent stuck" or "N agents stuck", and the row's tooltip SHALL say the same. The row SHALL keep its glyph, figure, progress track and needs-you badge. While the coordinator's or any of the project's agents' conversation is selected, the project row SHALL show as selected.

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

#### Scenario: The assistant is pinned
- **WHEN** the user named the assistant "Dot" and has nine projects
- **THEN** the sidebar shows "Dot" and "Boards" above the PROJECTS header, and dragging projects never moves them
