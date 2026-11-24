## REMOVED Requirements

### Requirement: Projects sharing a first word are shown as a group
**Reason**: First-word groups retire. Parent projects own their children and the Outcome lens draws them (see sidebar-lenses).
**Migration**: None needed. Projects keep their names and order; the human files them under parents with Move under… or by dragging in the Outcome lens.

### Requirement: Rows inside a group drop the repeated word
**Reason**: There are no groups to shorten names in. Every row shows its project's full name.
**Migration**: None needed.

### Requirement: A group row collapses and rolls up what is inside
**Reason**: Replaced by the collapsed parent row (see sidebar-lenses), which uses the same most-urgent-state rule.
**Migration**: None needed.

### Requirement: A Group button turns grouping on and off
**Reason**: Replaced by the lens control. The saved grouping choice in `desktop-preferences.json` is ignored.
**Migration**: The desktop reads an older preferences file and ignores its grouping key; the file's other choices are kept.

### Requirement: Projects are reordered by dragging
**Reason**: Its group rules retire with groups. "Projects are reordered by dragging in the Outcome lens" replaces it, with parents where groups were.
**Migration**: None needed; the stored order is unchanged.

## ADDED Requirements

### Requirement: Projects are reordered by dragging in the Outcome lens
In the Outcome lens (see sidebar-lenses), a person SHALL be able to drag project rows to reorder them, and each drop SHALL be sent to the host as one move, with the list redrawn from the order the host returns. A dragged row SHALL be droppable only within its own Space's section: over another Space's rows no insertion line SHALL show and a drop there SHALL move nothing, and over another Space's section row a hint SHALL read "Projects move between Spaces from their Overview." while the pointer is there. A move SHALL place the dragged projects among their own Space's projects and SHALL NOT change the relative order of any other Space's projects. A loose project or a parent row SHALL be droppable among the loose projects and parents of its section; dragging a parent SHALL move all its shown children with it as one block, and reordering children SHALL NOT move their parent. A child SHALL be droppable among its siblings, and drops that file or unfile a project SHALL follow sidebar-lenses. While a row is dragged, an insertion line SHALL show where it would land and the dragged row SHALL be dimmed. While a row is dragged and the pointer is near the top or bottom edge of the list, the list SHALL scroll in that direction, also while the pointer rests there, until it reaches its end; it SHALL stop scrolling when the pointer leaves that edge or the list, or the drag ends, and the insertion line SHALL keep to the row under the pointer as the list scrolls. A project row whose name is being edited SHALL NOT start a drag. A drag that left its row and is released back on it SHALL change nothing. Dragging SHALL NOT change what a click, a double-click or the actions menu does, including when the pointer moves a little between press and release without leaving the row.

#### Scenario: A loose project moves
- **WHEN** a Space's section lists loose projects A, B, C and C is dropped above A
- **THEN** the section lists C, A, B, and every other Space's projects keep their order

#### Scenario: A drop over another Space does nothing
- **WHEN** a Vyttle project is dragged over the Celadon section's row and released there
- **THEN** no insertion line shows, the hint "Projects move between Spaces from their Overview." shows under the Celadon row while the pointer is on it, and nothing moves

#### Scenario: A child moves among its siblings
- **WHEN** "Accessible contrast" has children "Dimmed inactive panes", "Colour-blind palettes", "Contrast warnings" and "Contrast warnings" is dropped above "Dimmed inactive panes"
- **THEN** the children read "Contrast warnings", "Dimmed inactive panes", "Colour-blind palettes" and "Accessible contrast" stays where it was

#### Scenario: A whole parent moves
- **WHEN** "Light themes", with two children, is dropped above "Accessible contrast" in the same section
- **THEN** "Light themes" and its children sit directly above "Accessible contrast" in the host's order, in the order they had

#### Scenario: A row is dropped at a place that was off-screen
- **WHEN** the list is taller than the sidebar and a row is dragged to its bottom edge and held there
- **THEN** the list scrolls down to its end, and the row can be dropped beside a row of its section that was not visible when the drag began

#### Scenario: A drag returned to its row is cancelled
- **WHEN** a parent row is dragged past other rows, brought back and released on itself
- **THEN** nothing moves and the parent stays expanded or collapsed as it was

#### Scenario: A click still selects
- **WHEN** a project row is clicked without moving the pointer
- **THEN** its conversation opens and nothing is moved

#### Scenario: A click that moves is still a click
- **WHEN** the pointer moves a few pixels between press and release without leaving the row
- **THEN** a project row is selected, a parent's chevron collapses or expands it, and the second click of a double-click opens rename, as each does without moving

## MODIFIED Requirements

### Requirement: Projects are listed in a section for each Space
The project list SHALL show, in every lens, a section for each Space that has something to show in that lens, in the order of Settings › Spaces, followed by a section named "No Space" for shown projects whose Space is not listed, such as those from a host older than Spaces. A Space with nothing to show SHALL have no section. Each section SHALL begin with a row showing a chevron, the Space's name in uppercase and how many shown projects it holds, and SHALL list its projects as the lens draws them, filtered to that Space. Activating the row SHALL collapse or expand the section, hiding or showing everything in it. A collapsed section SHALL show the most urgent state among its shown projects by the rule a collapsed parent row uses (see sidebar-lenses). Which sections are collapsed SHALL be kept in memory only, so an app restart starts them as the lens defines. Selecting a project inside a collapsed section or parent, such as from the Inbox or a notification, SHALL expand them so its row shows. Sections SHALL exist only in the desktop's drawing of the list: the host stores nothing new for them.

#### Scenario: Sections follow the Spaces' order
- **WHEN** Settings › Spaces lists Personal, Vyttle, Celadon and Work, and Work has no project
- **THEN** the list shows the sections PERSONAL, VYTTLE and CELADON in that order, and no section for Work

#### Scenario: Projects in no Space
- **WHEN** a project's Space is not among the listed Spaces
- **THEN** it is listed in a "NO SPACE" section after every Space's section

#### Scenario: A collapsed section rolls up
- **WHEN** the Personal section is collapsed and one of its projects has an item needing the human
- **THEN** its row shows the count of its projects and the needs-you count "1", and none of its projects are listed

#### Scenario: Selecting reveals
- **WHEN** a child of a collapsed parent in a collapsed section is selected from the Inbox
- **THEN** the section and the parent expand and the child's row shows selected

### Requirement: The sidebar lists projects only
Above the PROJECTS header, the sidebar SHALL show two pinned rows: the assistant, by the name the user gave it, and Boards. The assistant row SHALL show a working mark while any thread or job run is in a turn, and a count of job items needing attention (see scheduled-jobs); selecting it SHALL open the assistant with its thread tabs. Selecting Boards SHALL open the Boards library. The sidebar tree below SHALL list Space sections, the rows each lens draws (see sidebar-lenses) and project rows only, one project row for each coordinator in each place the lens lists it. It SHALL NOT list tickets, agents or threads, archived, legacy or retired ones included; the Agents panel lists agents (see work-progress) and the assistant lists threads. A loose project's or a child's row SHALL have no chevron and SHALL NOT collapse; a parent's row SHALL collapse its children. Its last line SHALL read "N agents working", counting the agents the Agents panel lists for the project's own tickets that are in a turn, and SHALL be left out when none is. When the stuck rule (see work-progress) flags any of the project's unarchived agents, that line SHALL also show a yellow pill reading "1 agent stuck" or "N agents stuck", and the row's tooltip SHALL say the same. The row SHALL keep its glyph, figure, progress track and needs-you badge, and a child's row its hollow badge for questions with its parent. While the coordinator's or any of the project's agents' conversation is selected, the project row SHALL show as selected.

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

#### Scenario: A parent counts only its own agents on its row
- **WHEN** a parent has one agent working on its own ticket and its child has two
- **THEN** the parent's row reads "1 agent working" and the child's "2 agents working"

#### Scenario: The assistant is pinned
- **WHEN** the user named the assistant "Wren" and has nine projects
- **THEN** the sidebar shows "Wren" and "Boards" above the PROJECTS header, and dragging projects never moves them
