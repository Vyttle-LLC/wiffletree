# processes-panel Specification

## Purpose
TBD - created by archiving change processes-panel. Update Purpose after archive.

## Requirements

### Requirement: The panel lists what agents have running
The host SHALL answer, for an agent, each process group its tools started that the record holds as running, and for a project coordinator the same for every agent of the project that is not archived, including agents with nothing running. A group SHALL be listed with its processes, their programs, process ids, start times and CPU time used, only while one of its recorded members, matched by process id and start time, is still alive in it. A group that still runs without such a member SHALL be listed as unconfirmed, without its processes. A group that no longer runs SHALL be forgotten and not listed.

#### Scenario: An agent left a group running
- **WHEN** an agent's turn left one `sleep` running in its own group
- **THEN** the agent's listing has that group, confirmed, with one process of the program `sleep` and its start time

#### Scenario: The coordinator's view
- **WHEN** the coordinator's panel is opened while one agent has a group running and another has none
- **THEN** both agents are listed, the second with no groups

#### Scenario: Members gone, id reused
- **WHEN** every recorded member of a group has exited and an unrelated process now has the group's id
- **THEN** the group is listed as unconfirmed with no processes

#### Scenario: Group gone
- **WHEN** a recorded group no longer runs
- **THEN** it is not listed and the record no longer holds it

### Requirement: The human stops a group from the panel
The host SHALL stop a group the human chose from the panel, or every group the panel lists, only through the confirmation rule that every other stop uses, and SHALL record each stopped group with the reason `stopped_by_human`. It SHALL queue for each agent whose processes it stopped one notice naming what was stopped, which rides along with the agent's next turn without starting one. A group that cannot be confirmed SHALL NOT be signalled. Stopping processes SHALL NOT pause or stop the agent.

#### Scenario: Stop on a row
- **WHEN** the human stops one of an agent's two groups
- **THEN** that group no longer runs, the other still does, the record says `stopped_by_human`, and the agent's conversation gains "Stopped 1 process … It was the program `sleep`." ending "stopped when the human pressed Stop in the Processes panel."

#### Scenario: Stop all
- **WHEN** the human presses Stop all on an agent's panel
- **THEN** every group of that agent is stopped and the panel lists none

#### Scenario: Unconfirmed group
- **WHEN** the human asks to stop a group no recorded member is alive in
- **THEN** nothing is signalled and no notice is added

#### Scenario: The agent keeps working
- **WHEN** the human stops an agent's processes
- **THEN** the agent's status and its project's running state are unchanged

### Requirement: Live values only while the panel is open
The desktop SHALL ask the host for the listing about once a second while the Processes panel is open on a session, and SHALL NOT ask while it is closed. It SHALL show each confirmed group's CPU as a percentage of one core, from the CPU time used between two readings by the processes present at both, and SHALL show no figure until it has two readings.

#### Scenario: Panel closed
- **WHEN** the Processes panel is not the open panel
- **THEN** no listing is requested and the process table is not read for it

#### Scenario: Eight busy processes
- **WHEN** eight processes of one group each used 0.85 seconds of CPU between two readings one second apart
- **THEN** the group's CPU reads 680% and is highlighted

### Requirement: The rail shows a count
The Processes entry on the rail SHALL carry the number of processes the record holds as running for the panel's agents, read from the snapshot without reading the process table, and SHALL carry no count when there are none.

#### Scenario: Coordinator's count
- **WHEN** one agent of the project has 8 processes recorded and the coordinator has 1
- **THEN** the coordinator's rail entry shows 9

### Requirement: Counts and programs in words
The panel SHALL state counts in words before names ("8 processes", "1 process", "8 processes running", "Nothing running") and SHALL name programs as programs, never as a name beside a bare count such as "name × N" or "name (N)".

#### Scenario: A group of eight
- **WHEN** a group holds eight processes of the program `yes`
- **THEN** its row reads `yes` with "8 processes · started HH:MM · group G" beneath
