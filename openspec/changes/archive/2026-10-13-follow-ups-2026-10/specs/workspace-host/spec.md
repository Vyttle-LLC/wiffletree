## MODIFIED Requirements

### Requirement: Startup stops provider processes the last host left running
Each run SHALL record its provider's process group and that group leader's start time. At startup, for every run the last host left unfinished whose process group still runs, the host SHALL stop the group the way a turn's end does, SIGTERM, a grace period and then SIGKILL, and SHALL then mark the run interrupted and record when the process was stopped. The host SHALL send each signal only while the group's leader has the recorded process id and the recorded start time. When it cannot confirm that, because no start time was recorded, the start time differs or the leader has exited and other members remain, it SHALL signal nothing, SHALL still mark the run interrupted, and SHALL raise one inbox note on the session that names the session and the process id. When the group still runs after the host signalled it, as when its leader exits on SIGTERM and other members ignore it, the host SHALL NOT signal the group again without confirming its leader, SHALL record when the process was stopped as for any stopped leftover, and SHALL raise the note saying that it signalled the group and that part of it still runs, not that it did not stop the process.

#### Scenario: Leftover provider
- **WHEN** the host starts and an unfinished run's provider process is still running with its recorded start time
- **THEN** the process group is stopped before any turn starts
- **AND** the run is interrupted and records when its process was stopped

#### Scenario: Recycled process id
- **WHEN** the host starts and an unfinished run's process id now belongs to a process with a different start time
- **THEN** that process receives no signal
- **AND** one inbox note names the session and the process id

#### Scenario: Run recorded without a start time
- **WHEN** the host starts and an unfinished run recorded before start times were kept has a process group that still runs
- **THEN** the group receives no signal and one inbox note names the session and the process id

#### Scenario: Process already gone
- **WHEN** the host starts and an unfinished run's process group no longer runs
- **THEN** the run is interrupted, nothing is signalled and no note is raised

#### Scenario: The provider exits and a group member survives
- **WHEN** the host starts, signals a confirmed leftover, its leader exits on SIGTERM and another member of the group keeps running
- **THEN** the run is interrupted and records when its process was stopped
- **AND** one inbox note says the group was signalled and that part of it still runs

### Requirement: No turn starts beside a leftover process
While a session's leftover-process note is open and the process group it names still runs, the host SHALL start no turn for that session. It SHALL hold the turn as other held turns are, with the reason naming the process id, and SHALL check again without waiting for another event. When the group no longer runs, the host SHALL settle the note itself and start the turn. The host SHALL settle a note once it sees its group gone whether or not the session has a turn due, and SHALL look again without waiting for another event while a note is open, so no note keeps saying a process runs after it exited. When the human dismisses the note, the hold SHALL end. The hold SHALL NOT depend on startup having raised the note: when a turn is due for a session, the host SHALL look once at each of its finished runs whose end no host saw, those marked interrupted or finished by a quit without the process having been stopped, such as an earlier version left them. For a run whose process group still runs it SHALL raise the note and hold the session; a run whose group no longer runs SHALL be settled and never looked at again, since process ids are reused. A run whose end its provider reported, and one whose process startup stopped or found gone, SHALL be settled at that point.

#### Scenario: Retry while the process runs
- **WHEN** an interrupted session is retried while the process group its note names still runs
- **THEN** no turn starts and the session shows as blocked with a reason naming the process id

#### Scenario: The process exits
- **WHEN** that process group exits
- **THEN** the host settles the note and starts the session's turn

#### Scenario: Run an earlier version marked interrupted
- **WHEN** a session has a finished, interrupted run with no note whose process group still runs, and input is queued for it
- **THEN** no turn starts, one inbox note names the session and the process id, and the turn starts once the group exits

#### Scenario: Process id reused later
- **WHEN** a settled run's process id is later given to another process
- **THEN** the session is not held and no note is raised

#### Scenario: The human dismisses the note
- **WHEN** the human dismisses the note because the process is not the agent's
- **THEN** the session's turn starts

#### Scenario: The process exits while no turn is due
- **WHEN** the process group a note names exits while its session is disconnected with its input held
- **THEN** the host settles the note without the session being retried
