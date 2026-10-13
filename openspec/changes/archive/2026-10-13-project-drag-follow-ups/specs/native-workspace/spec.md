## MODIFIED Requirements

### Requirement: Projects are reordered by dragging
A person SHALL be able to drag project rows to reorder them, and each drop SHALL be sent to the host as one move, with the list redrawn from the order the host returns. While grouping is off, any project row SHALL be droppable at any place in the flat list. While grouping is on, a project inside a group SHALL be droppable only within that group, and a group row or an ungrouped project row SHALL be droppable only among the other groups and ungrouped projects; dragging a group SHALL move all its shown members as one block, and reordering inside a group SHALL NOT move the group. While a row is dragged, an insertion line SHALL show where it would land and the dragged row SHALL be dimmed. While a row is dragged and the pointer is near the top or bottom edge of the list, the list SHALL scroll in that direction, also while the pointer rests there, until it reaches its end; it SHALL stop scrolling when the pointer leaves that edge or the list, or the drag ends, and the insertion line SHALL keep to the row under the pointer as the list scrolls. Ticket rows and agent rows SHALL NOT be draggable, and a project row whose name is being edited SHALL NOT start a drag. A drag that left its row and is released back on it SHALL change nothing. Dragging SHALL NOT change what a click, a double-click, the chevron or the actions menu does, including when the pointer moves a little between press and release without leaving the row.

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
- **THEN** a project row is selected, a group row is collapsed or expanded, a project's chevron collapses or expands it, and the second click of a double-click opens rename, as each does without moving
