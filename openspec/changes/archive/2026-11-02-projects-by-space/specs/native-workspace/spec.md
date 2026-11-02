## ADDED Requirements

### Requirement: Projects are listed in a section for each Space
The project list SHALL show a section for each Space that has a shown project, in the order of Settings › Spaces, followed by a section named "No Space" for shown projects whose Space is not listed, such as those from a host older than Spaces. A Space with no shown project SHALL have no section. Sections SHALL show whether grouping is on or off. Each section SHALL begin with a row showing a chevron, the Space's name in uppercase and how many shown projects it holds, and SHALL list its projects in the stored order, filtered to that Space. Activating the row SHALL collapse or expand the section, hiding or showing its projects and groups. A collapsed section SHALL show the most urgent state among its shown projects by the rule a collapsed group uses. Which sections are collapsed SHALL be kept in memory only, so an app restart starts them expanded. Selecting a project inside a collapsed section or group, such as from the Inbox or a notification, SHALL expand them so its row shows. Sections SHALL exist only in the desktop's drawing of the list: the host stores nothing new for them.

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

## MODIFIED Requirements

### Requirement: Projects sharing a first word are shown as a group
A project's first word SHALL be the leading run of its name up to the first whitespace, with trailing characters that are neither letters nor digits removed, compared without regard to case. While grouping is on, two or more shown projects of one Space's section with the same first word SHALL appear under one group row in that section, and every other project SHALL remain an ordinary row with its full name. Projects in different sections SHALL NOT share a group. Groups SHALL be computed over the projects currently shown, so archived projects count only while Archived is on. A group SHALL sit where its topmost member is in the stored order, its members SHALL keep their stored order, and its label SHALL be the first word as written in its topmost member. Groups SHALL exist only in the desktop's drawing of the list: the host stores none, and a person SHALL NOT be able to create, rename or delete one. While grouping is off each section SHALL list its projects as ordinary rows, with full names, in the same stored order.

#### Scenario: Case and a colon do not matter
- **WHEN** grouping is on and the shown projects of one Space are "Wiffletree: a" and "wiffletree b"
- **THEN** both appear under one group labelled "Wiffletree"

#### Scenario: The same word in two Spaces
- **WHEN** grouping is on, "wiffletree: a" and "wiffletree: c" are in Vyttle, and "wiffletree: b" and "wiffletree: d" are in Personal
- **THEN** each section has its own group "wiffletree" holding its own two projects

#### Scenario: A single project stays an ordinary row
- **WHEN** grouping is on and only "ci-runners: Autoscaling" starts with "ci-runners"
- **THEN** it is an ordinary row reading "ci-runners: Autoscaling"

#### Scenario: A group sits at its topmost member
- **WHEN** the stored order of one Space's projects is "Trial copy", "infra: State split", "watch: Relay", "infra: Neptune"
- **THEN** its section reads "Trial copy", the group "infra" holding "State split" then "Neptune", then "watch: Relay"

#### Scenario: Archived projects join only while shown
- **WHEN** "infra: Neptune" is archived and "infra: State split" is not
- **THEN** with Archived off "infra: State split" is an ordinary row, and with Archived on both sit under the group "infra"

#### Scenario: Grouping off
- **WHEN** grouping is turned off
- **THEN** every project is an ordinary row with its full name under its Space's section, in the stored order

#### Scenario: Renaming moves a project between groups
- **WHEN** "infra: Neptune" is renamed "watch: Neptune" while "watch: Relay" is shown in the same Space
- **THEN** it appears under the group "watch"

### Requirement: A Group button turns grouping on and off
The PROJECTS header SHALL carry a button labelled "Group" with a folder icon, to the left of Archived, drawn pressed while grouping is on, with the tooltip "Group projects by first word". Grouping SHALL be on by default. The choice SHALL be saved with the desktop's other view choices in `desktop-preferences.json`, a file without the choice SHALL read as on, and changing it SHALL NOT change the stored order or the Space sections.

#### Scenario: The choice survives a relaunch
- **WHEN** grouping is turned off and the app is relaunched
- **THEN** each section lists its projects as ordinary rows and the button is not pressed

#### Scenario: An older preferences file
- **WHEN** `desktop-preferences.json` was saved before the choice existed
- **THEN** grouping is on and the file's other choices are kept

### Requirement: Projects are reordered by dragging
A person SHALL be able to drag project rows to reorder them, and each drop SHALL be sent to the host as one move, with the list redrawn from the order the host returns. A dragged row SHALL be droppable only within its own Space's section: over another Space's rows no insertion line SHALL show and a drop there SHALL move nothing, and over another Space's section row a hint SHALL read "Projects move between Spaces from their Overview." while the pointer is there. A move SHALL place the dragged projects among their own Space's projects and SHALL NOT change the relative order of any other Space's projects. While grouping is off, any project row SHALL be droppable at any place in its section. While grouping is on, a project inside a group SHALL be droppable only within that group, and a group row or an ungrouped project row SHALL be droppable only among the other groups and ungrouped projects of its section; dragging a group SHALL move all its shown members as one block, and reordering inside a group SHALL NOT move the group. While a row is dragged, an insertion line SHALL show where it would land and the dragged row SHALL be dimmed. While a row is dragged and the pointer is near the top or bottom edge of the list, the list SHALL scroll in that direction, also while the pointer rests there, until it reaches its end; it SHALL stop scrolling when the pointer leaves that edge or the list, or the drag ends, and the insertion line SHALL keep to the row under the pointer as the list scrolls. A project row whose name is being edited SHALL NOT start a drag. A drag that left its row and is released back on it SHALL change nothing. Dragging SHALL NOT change what a click, a double-click or the actions menu does, including when the pointer moves a little between press and release without leaving the row.

#### Scenario: A project moves within the flat list
- **WHEN** grouping is off, a Space's section lists A, B, C and C is dropped above A
- **THEN** the section lists C, A, B, and every other Space's projects keep their order

#### Scenario: A drop over another Space does nothing
- **WHEN** a Vyttle project is dragged over the Sage section's row and released there
- **THEN** no insertion line shows, the hint "Projects move between Spaces from their Overview." shows under the Sage row while the pointer is on it, and nothing moves

#### Scenario: A project moves within its group
- **WHEN** the group "wiffletree" holds "Reorder", "Owners", "Compact" and "Compact" is dropped above "Reorder"
- **THEN** the group holds "Compact", "Reorder", "Owners" and stays where it was among the other rows

#### Scenario: The top project of a group moves down
- **WHEN** the group "wiffletree" sits above "ci-runners: Autoscaling" and its topmost project is dropped at the bottom of the group
- **THEN** the group still sits above "ci-runners: Autoscaling"

#### Scenario: A whole group moves
- **WHEN** the group "watch", holding two projects, is dropped above the group "wiffletree" in the same section
- **THEN** both of its projects sit directly above the topmost "wiffletree" project in the host's order, in the order they had

#### Scenario: A grouped project cannot leave its group
- **WHEN** a project inside the group "wiffletree" is dragged over a row outside that group
- **THEN** no insertion line appears there and dropping it there moves nothing

#### Scenario: A row is dropped at a place that was off-screen
- **WHEN** the list is taller than the sidebar and a row is dragged to its bottom edge and held there
- **THEN** the list scrolls down to its end, and the row can be dropped beside a row of its section that was not visible when the drag began

#### Scenario: A drag returned to its row is cancelled
- **WHEN** a group row is dragged past other rows, brought back and released on itself
- **THEN** nothing moves and the group stays expanded or collapsed as it was

#### Scenario: A click still selects
- **WHEN** a project row is clicked without moving the pointer
- **THEN** its conversation opens and nothing is moved

#### Scenario: A click that moves is still a click
- **WHEN** the pointer moves a few pixels between press and release without leaving the row
- **THEN** a project row is selected, a group row is collapsed or expanded, and the second click of a double-click opens rename, as each does without moving
