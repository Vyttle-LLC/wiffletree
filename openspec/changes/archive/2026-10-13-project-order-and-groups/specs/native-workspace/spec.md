## ADDED Requirements

### Requirement: Projects are listed in the stored order
The sidebar SHALL list projects in the order the host returns them, and SHALL NOT sort them itself.

#### Scenario: The list follows a move
- **WHEN** the host's order changes from A, B, C to C, A, B
- **THEN** the sidebar lists C, A, B

### Requirement: Projects sharing a first word are shown as a group
A project's first word SHALL be the leading run of its name up to the first whitespace, with trailing characters that are neither letters nor digits removed, compared without regard to case. While grouping is on, two or more shown projects with the same first word SHALL appear under one group row, and every other project SHALL remain an ordinary row with its full name. Groups SHALL be computed over the projects currently shown, so archived projects count only while Archived is on. A group SHALL sit where its topmost member is in the stored order, its members SHALL keep their stored order, and its label SHALL be the first word as written in its topmost member. Groups SHALL exist only in the desktop's drawing of the list: the host stores none, and a person SHALL NOT be able to create, rename or delete one. While grouping is off the list SHALL be flat, with full names, in the same stored order.

#### Scenario: Case and a colon do not matter
- **WHEN** grouping is on and the shown projects are "Wiffletree: a" and "wiffletree b"
- **THEN** both appear under one group labelled "Wiffletree"

#### Scenario: A single project stays an ordinary row
- **WHEN** grouping is on and only "ci-runners: Autoscaling" starts with "ci-runners"
- **THEN** it is an ordinary row reading "ci-runners: Autoscaling"

#### Scenario: A group sits at its topmost member
- **WHEN** the stored order is "Trial copy", "infra: State split", "watch: Relay", "infra: Neptune"
- **THEN** the list reads "Trial copy", the group "infra" holding "State split" then "Neptune", then "watch: Relay"

#### Scenario: Archived projects join only while shown
- **WHEN** "infra: Neptune" is archived and "infra: State split" is not
- **THEN** with Archived off "infra: State split" is an ordinary row, and with Archived on both sit under the group "infra"

#### Scenario: Grouping off
- **WHEN** grouping is turned off
- **THEN** every project is an ordinary row with its full name, in the stored order

#### Scenario: Renaming moves a project between groups
- **WHEN** "infra: Neptune" is renamed "watch: Neptune" while "watch: Relay" is shown
- **THEN** it appears under the group "watch"

### Requirement: Rows inside a group drop the repeated word
A project row inside a group SHALL show its name without the first word and the separator that follows it, and SHALL show the full name when nothing else remains. Its tooltip, the conversation's title and the name being edited in an inline rename SHALL keep the full name.

#### Scenario: The word and its colon are dropped
- **WHEN** "wiffletree: Repository owners" sits in the group "wiffletree"
- **THEN** its row reads "Repository owners" and its tooltip begins "wiffletree: Repository owners"

#### Scenario: Nothing remains
- **WHEN** a project named "wiffletree" sits in the group "wiffletree"
- **THEN** its row reads "wiffletree"

### Requirement: A group row collapses and rolls up what is inside
A group row SHALL show a chevron, a folder icon, the group's label and how many projects it holds, and activating it SHALL collapse or expand the group as a project row's chevron does its project. A collapsed group SHALL show the most urgent state among its shown members: the number of items that need the human when there are any, otherwise blocked when a member's row would show blocked, otherwise working when a member's row would show working, and otherwise nothing.

#### Scenario: Needs-you outranks blocked and working
- **WHEN** a collapsed group holds one project with one item needing the human, one blocked project and one working project
- **THEN** the group row shows the needs-you count "1"

#### Scenario: Blocked outranks working
- **WHEN** a collapsed group holds a blocked project and a working project and nothing needs the human
- **THEN** the group row shows the blocked mark

#### Scenario: A quiet group
- **WHEN** every project in a collapsed group is ready, done or paused
- **THEN** the group row shows its count and no state

### Requirement: A Group button turns grouping on and off
The PROJECTS header SHALL carry a button labelled "Group" with a folder icon, to the left of Archived, drawn pressed while grouping is on, with the tooltip "Group projects by first word". Grouping SHALL be on by default. The choice SHALL be saved with the desktop's other view choices in `desktop-preferences.json`, a file without the choice SHALL read as on, and changing it SHALL NOT change the stored order.

#### Scenario: The choice survives a relaunch
- **WHEN** grouping is turned off and the app is relaunched
- **THEN** the list is flat and the button is not pressed

#### Scenario: An older preferences file
- **WHEN** `desktop-preferences.json` was saved before the choice existed
- **THEN** grouping is on and the file's other choices are kept

### Requirement: Projects are reordered by dragging
A person SHALL be able to drag project rows to reorder them, and each drop SHALL be sent to the host as one move, with the list redrawn from the order the host returns. While grouping is off, any project row SHALL be droppable at any place in the flat list. While grouping is on, a project inside a group SHALL be droppable only within that group, and a group row or an ungrouped project row SHALL be droppable only among the other groups and ungrouped projects; dragging a group SHALL move all its shown members as one block, and reordering inside a group SHALL NOT move the group. While a row is dragged, an insertion line SHALL show where it would land and the dragged row SHALL be dimmed. Ticket rows and agent rows SHALL NOT be draggable, and a project row whose name is being edited SHALL NOT start a drag. Dragging SHALL NOT change what a click, a double-click, the chevron or the actions menu does.

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

#### Scenario: A click still selects
- **WHEN** a project row is clicked without moving the pointer
- **THEN** its conversation opens and nothing is moved
