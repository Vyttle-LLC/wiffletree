## ADDED Requirements

### Requirement: Projects have one manual order
The host SHALL store one flat order of projects that contains every project exactly once, and SHALL return projects in that order, topmost first, in every snapshot. A new project SHALL be placed at the top. The order SHALL be kept in the store, so it is the same after a restart and for every client. A store from before this order existed SHALL open with its projects newest first, as they were listed until then, and with nothing else changed.

#### Scenario: A new project lands at the top
- **WHEN** the order is "Calls", "Billing" and a project "Alerts" is created
- **THEN** a snapshot lists "Alerts", "Calls", "Billing"

#### Scenario: The order survives a restart
- **WHEN** projects are moved into the order "Billing", "Alerts", "Calls" and the host restarts
- **THEN** a snapshot lists "Billing", "Alerts", "Calls"

#### Scenario: An existing store opens newest first
- **WHEN** a store from before the order existed holds "Calls", then "Billing", then "Alerts", created in that sequence
- **THEN** it opens listing "Alerts", "Billing", "Calls", and every project, session, ticket and message is as it was

### Requirement: Moving projects in the order
The host SHALL offer one command that moves one or more projects to just before a named project, or to the end of the order when none is named. The moved projects SHALL end up next to each other in the relative order they had before the move, and every other project SHALL keep its relative order. The host SHALL refuse a move that lists no project, lists a project twice, names a project that does not exist, or names one of the moved projects as the place to move before; a refused move SHALL leave the order unchanged.

#### Scenario: One project moves up
- **WHEN** the order is A, B, C, D and D is moved before B
- **THEN** the order is A, D, B, C

#### Scenario: A block moves and keeps its own order
- **WHEN** the order is A, B, C, D, E and the projects D and B are moved before A
- **THEN** the order is B, D, A, C, E

#### Scenario: A move to the end
- **WHEN** the order is A, B, C and A is moved with no project to move before
- **THEN** the order is B, C, A

#### Scenario: An unknown project is refused
- **WHEN** a move lists a project that does not exist, or names one as the place to move before
- **THEN** the host refuses it and the order is unchanged
