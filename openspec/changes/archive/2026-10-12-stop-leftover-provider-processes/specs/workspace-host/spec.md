## ADDED Requirements

### Requirement: A quit stops running turns
The host SHALL offer a shutdown that cancels every running turn and returns only once each of those turns' provider process groups has been stopped, as at any turn's end, or a bounded wait has passed. The desktop app SHALL call it before it exits from a quit, and the headless host SHALL call it when its input closes. A run SHALL be recorded as finished only after its process group has been stopped; a turn still running when the wait ends SHALL stay unfinished, so the next start handles its process.

#### Scenario: Quit during a turn
- **WHEN** the host is shut down while a provider is mid-turn
- **THEN** the provider's process group no longer runs when the shutdown returns
- **AND** the run is finished with the quit as its error and its input is held for a retry

#### Scenario: Quit with no turn running
- **WHEN** the host is shut down while no turn runs
- **THEN** the shutdown returns without waiting

### Requirement: Startup stops provider processes the last host left running
Each run SHALL record its provider's process group and that group leader's start time. At startup, for every run the last host left unfinished whose process group still runs, the host SHALL stop the group the way a turn's end does, SIGTERM, a grace period and then SIGKILL, and SHALL then mark the run interrupted and record when the process was stopped. The host SHALL send each signal only while the group's leader has the recorded process id and the recorded start time. When it cannot confirm that, because no start time was recorded, the start time differs or the leader has exited and other members remain, it SHALL signal nothing, SHALL still mark the run interrupted, and SHALL raise one inbox note on the session that names the session and the process id.

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

### Requirement: No turn starts beside a leftover process
While a session's leftover-process note is open and the process group it names still runs, the host SHALL start no turn for that session. It SHALL hold the turn as other held turns are, with the reason naming the process id, and SHALL check again without waiting for another event. When the group no longer runs, the host SHALL settle the note itself and start the turn. When the human dismisses the note, the hold SHALL end. The hold SHALL NOT depend on startup having raised the note: when a turn is due for a session, the host SHALL look once at each of its finished runs whose end no host saw, those marked interrupted or finished by a quit without the process having been stopped, such as an earlier version left them. For a run whose process group still runs it SHALL raise the note and hold the session; a run whose group no longer runs SHALL be settled and never looked at again, since process ids are reused. A run whose end its provider reported, and one whose process startup stopped or found gone, SHALL be settled at that point.

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

### Requirement: The retry note names a process stopped at startup
When startup stopped an interrupted run's leftover process, the note an agent gets for that run, on a retry of its input or on its first turn after the restart, SHALL say that the process kept running after the host stopped and when it was stopped, so the agent checks the worktree for work done after the turn's last recorded step.

#### Scenario: Retry after a leftover was stopped
- **WHEN** an agent retries the input of a run whose leftover process startup stopped
- **THEN** its prompt names the run and says its process was stopped at startup, with the time

#### Scenario: Retry with no leftover
- **WHEN** an agent retries the input of an interrupted run whose process had already exited
- **THEN** its prompt names the run and says nothing about a stopped process
