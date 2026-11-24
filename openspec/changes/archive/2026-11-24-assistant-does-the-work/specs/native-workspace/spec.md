## MODIFIED Requirements

### Requirement: The sidebar lists projects only
Above the PROJECTS header, the sidebar SHALL show two pinned rows: the assistant, by the name the user gave it, and Boards. The assistant row SHALL show a working mark while the Assistant, any of its work items, an issue thread or a job run is in a turn, and a count of the items its rail lists under Needs you (see assistant); selecting it SHALL open the Assistant's conversation with its rail. Selecting Boards SHALL open the Boards library. The sidebar tree below SHALL list group rows and project rows only, one project row for each coordinator whose project is not an Assistant work item. It SHALL NOT list tickets, agents, threads or Assistant work, archived, legacy or retired ones included; the Agents panel lists agents (see work-progress) and the Assistant's rail lists its work. A project row SHALL have no chevron and SHALL NOT collapse. Its last line SHALL read "N agents working", counting the agents the Agents panel lists for the project that are in a turn, and SHALL be left out when none is. When the stuck rule (see work-progress) flags any of the project's unarchived agents, that line SHALL also show a yellow pill reading "1 agent stuck" or "N agents stuck", and the row's tooltip SHALL say the same. The row SHALL keep its glyph, figure, progress track and needs-you badge. While the coordinator's or any of the project's agents' conversation is selected, the project row SHALL show as selected; while an Assistant work item's project view is open, the assistant row SHALL show as selected.

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
- **WHEN** the user named the assistant "Wren" and has nine projects
- **THEN** the sidebar shows "Wren" and "Boards" above the PROJECTS header, and dragging projects never moves them

#### Scenario: Assistant work is not a project row
- **WHEN** the human started "Website redesign" and the Assistant runs two work items, one waiting on a decision
- **THEN** Projects lists only "Website redesign", and the assistant row shows a working mark and the count 1

#### Scenario: Open details
- **WHEN** the human chooses Open details on a work card
- **THEN** the work's project view opens with Back to chat, and the assistant row shows as selected

### Requirement: Projects are listed in a section for each Space
The project list SHALL show a section for each Space that has a shown project, in the order of Settings › Spaces, followed by a section named "No Space" for shown projects whose Space is not listed, such as those from a host older than Spaces. A shown project SHALL be one the sidebar lists, so an Assistant work item is never shown. A Space with no shown project SHALL have no section. Sections SHALL show whether grouping is on or off. Each section SHALL begin with a row showing a chevron, the Space's name in uppercase and how many shown projects it holds, and SHALL list its projects in the stored order, filtered to that Space. Activating the row SHALL collapse or expand the section, hiding or showing its projects and groups. A collapsed section SHALL show the most urgent state among its shown projects by the rule a collapsed group uses. Which sections are collapsed SHALL be kept in memory only, so an app restart starts them expanded. Selecting a project inside a collapsed section or group, such as from the Inbox or a notification, SHALL expand them so its row shows; selecting an Assistant work item from the Inbox SHALL open its project view from the Assistant instead. Sections SHALL exist only in the desktop's drawing of the list: the host stores nothing new for them.

#### Scenario: Sections follow the Spaces' order
- **WHEN** Settings › Spaces lists Personal, Vyttle, Sage and Work, and Work has no project
- **THEN** the list shows the sections PERSONAL, VYTTLE and SAGE in that order, and no section for Work

#### Scenario: Projects in no Space
- **WHEN** a project's Space is not among the listed Spaces
- **THEN** it is listed in a "NO SPACE" section after every Space's section

#### Scenario: A collapsed section rolls up
- **WHEN** the Personal section is collapsed and one of its projects has an item needing the human
- **THEN** its row shows the count of its projects and the needs-you count "1", and none of its projects are listed

#### Scenario: Selecting reveals
- **WHEN** a project inside a collapsed group of a collapsed section is selected from the Inbox
- **THEN** the section and the group expand and the project's row shows selected

#### Scenario: Only Assistant work in a Space
- **WHEN** Celadon's only projects are Assistant work items
- **THEN** the list has no CELADON section
