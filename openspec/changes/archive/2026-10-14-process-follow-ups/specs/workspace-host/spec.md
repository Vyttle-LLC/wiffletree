## MODIFIED Requirements

### Requirement: A started process holds only its own descriptors
A process the host starts, whether a provider turn, a compaction, a provider probe or a Git or `gh` call, and a process the desktop app starts while the host runs in it, such as the update's download, unpacking, signature checks and the shell that swaps the bundle, SHALL hold no file descriptor of the host's other than its own standard input, output and error. This SHALL hold when other threads are starting processes at the same moment, so that neither the process nor anything it starts can keep another process's pipe open. A provider SHALL still receive its three pipes and the `WORKSPACE_SOCKET` and `WORKSPACE_TOKEN` environment.

#### Scenario: Providers started together
- **WHEN** the host starts several providers at the same moment while it also runs a Git query
- **THEN** each provider holds its standard input, output and error and no other descriptor of the host's

#### Scenario: A provider leaves a process running
- **WHEN** two providers start together and one leaves a long-lived process behind
- **THEN** the other provider still sees the end of its prompt, because that process holds neither end of its input pipe

#### Scenario: The desktop app's update commands
- **WHEN** the desktop app starts an update command while a descriptor of its process is still inheritable, as a provider's pipe is between being made and marked
- **THEN** the command does not hold that descriptor

#### Scenario: A descriptor list that may be cut short
- **WHEN** a started process cannot read its whole descriptor list, because the list call failed or filled its buffer
- **THEN** it still holds only its standard input, output and error

### Requirement: A process group is signalled only while a recorded member is alive in it
The host SHALL signal a recorded process group only while at least one of its recorded members, matched by process id and start time, is alive and still in that group. It SHALL NOT require the group's leader to be alive and SHALL NOT signal on a group id alone. It SHALL send SIGTERM and then SIGCONT, so that a stopped process that handles SIGTERM acts on it, wait the grace period, and send SIGKILL only if the group still runs and a process it confirmed is still alive in it. The same check SHALL be used when an agent is archived, when it is stopped, at a quit and at startup. A recorded group that no longer runs SHALL be dropped from the record with no signal. A group that still runs and cannot be confirmed SHALL receive no signal, SHALL stay recorded as running, and SHALL raise one inbox note on the agent that names the agent and the process group and says the host did not stop it. A group that still runs after the host signalled it, because processes joined it after the host confirmed it or outlast SIGKILL, SHALL NOT be signalled again while no process the host confirmed is alive in it, SHALL stay recorded as running, and SHALL raise the note saying that the host signalled the group and that part of it still runs, not that it did not stop it. No note SHALL be raised when nothing still runs. Each group the host stops SHALL be recorded with the time and one reason: `archived`, `archive_failed`, `stopped`, `quit` or `startup`.

#### Scenario: A group left frozen
- **WHEN** the host stops a confirmed group whose processes are stopped, as a host that died while it froze a provider's group leaves them, and the group's leader handles SIGTERM
- **THEN** the leader's handler runs and the group exits within the grace period, without SIGKILL

#### Scenario: Orphans of a leader that exited
- **WHEN** the host stops a recorded group whose leader has exited and whose recorded members still run
- **THEN** every process in the group is stopped
- **AND** the record says when and why

#### Scenario: A process that ignores SIGTERM
- **WHEN** a confirmed group still runs after the grace period
- **THEN** it is killed

#### Scenario: A member that ignores SIGTERM beside a leader that exits on it
- **WHEN** only a group's leader is recorded, the leader exits on SIGTERM and another process that was in the group when the host confirmed it ignores the signal
- **THEN** that process is killed and the group is recorded as stopped

#### Scenario: A process that joined after the group was confirmed
- **WHEN** every process the host confirmed exits on SIGTERM and a process that joined the group after that keeps running
- **THEN** the group receives no second signal and stays recorded as running
- **AND** one inbox note says the host signalled the group and that part of it still runs

#### Scenario: Recycled group id
- **WHEN** every recorded member of a group has gone and the group's id now belongs to an unrelated process
- **THEN** that process receives no signal
- **AND** one inbox note names the agent and the process group

#### Scenario: Nothing left
- **WHEN** every recorded member of a group has gone and no process has that group id
- **THEN** nothing is signalled, no note is raised and the group is no longer recorded

### Requirement: Archiving an agent stops what it started
When a session is archived, by any path, the host SHALL stop every process group recorded for that session across all its turns, under the confirmation rule, and SHALL do so before any ticket worktree is removed. The paths are the human archiving an agent or a project, `accept_ticket`, `close_ticket`, `archive_agent`, verification retiring a verifier and `verify_ticket` replacing one. A session archived while its turn still runs SHALL have its groups stopped when that turn ends. An archive that is refused because a worktree cannot be removed SHALL stop nothing. `verify_ticket` SHALL stop what a verifier it replaces started only once the new review has started; a `verify_ticket` that fails SHALL leave the verifier it would have replaced unarchived and its processes running, with no record of a stop and no notice. When an archive, a close or an accept fails after that check, what it stopped stays stopped: each such group SHALL be recorded with the reason `archive_failed` instead of `archived`, and each agent that is still unarchived SHALL receive the notice of what was stopped, saying that the archive then failed, with its next turn.

#### Scenario: Accepting a ticket
- **WHEN** a ticket is accepted and its implementer left a process running
- **THEN** the process is stopped before the ticket's worktree is removed
- **AND** its group is recorded as stopped with the reason `archived`

#### Scenario: Closing a ticket
- **WHEN** a ticket is closed and one of its agents left a process running
- **THEN** the process is stopped

#### Scenario: The coordinator archives an agent
- **WHEN** `archive_agent` archives a reviewer that left a process running
- **THEN** the process is stopped

#### Scenario: A verifier is retired
- **WHEN** verification archives a verifier that left a process running
- **THEN** the process is stopped, at once if its turn has ended and otherwise when that turn ends

#### Scenario: The human archives a project
- **WHEN** the human archives a project whose agents left processes running
- **THEN** every one of them is stopped

#### Scenario: Refused archive
- **WHEN** an archive is refused because a worktree holds uncommitted work
- **THEN** no process is signalled

#### Scenario: An archive that fails after the check
- **WHEN** a ticket's close passes the worktree check, stops a process its agent left running, and then fails
- **THEN** the agent is still unarchived and the process stays stopped
- **AND** its group is recorded as stopped with the reason `archive_failed`
- **AND** the agent's next turn takes the notice, which says the process was stopped when the agent was to be archived and that this then failed

#### Scenario: A review that fails after replacing a verifier
- **WHEN** `verify_ticket` replaces a verifier that left a process running and then fails before the review starts
- **THEN** the verifier is still unarchived, its process still runs and nothing records or announces a stop
- **AND** when a later `verify_ticket` starts the review, the process is stopped with the reason `archived`

### Requirement: Stopping an agent stops what it started
When the human pauses a session, when the human stops a project, and when a coordinator calls `stop_agents`, the host SHALL stop every process group recorded for each agent it stops, under the confirmation rule, before the command or the tool call returns. It SHALL first look afresh at what each stopped agent's running turn has started, without waiting for that turn to pass it on, and SHALL record every process then followed for that turn, not only what is new since the turn last asked, so a process started a moment before the stop is stopped too, also when its sighting was on its way to the record. The watcher SHALL keep answering for a turn until the host has stored everything that turn sent, its last sightings included, so the look names what the turn started also after the provider has exited and while the turn's end is on its way. A turn that ends on its own as its agent is stopped SHALL be treated as a stopped turn: what it started SHALL be stopped and SHALL NOT be announced as left running. A stop lets a compaction that has started finish, so it SHALL leave alone what that compaction's provider started, such as a Codex app-server's tool servers; when the compaction ends and its agent is still paused or its project still stopped, what the compaction left running SHALL be stopped then. Stopping a project SHALL do so for every agent of the project, idle ones included: an emergency stop leaves nothing running unsupervised. Before it signals a turn's own provider group, the host SHALL stop that group with SIGSTOP, wait for its processes to stop, and look at what the turn started while the group is stopped, so that a tool the provider forked a moment before, which the signal would orphan, is recorded; it SHALL then send SIGTERM and continue the group, and SHALL continue it also when the look fails. When a cancelled turn ends, the host SHALL stop the groups recorded for its agent since. Each group SHALL be recorded as stopped with the reason `stopped`.

#### Scenario: Pausing an idle agent
- **WHEN** the human pauses an agent that left a process running in an earlier turn
- **THEN** the process no longer runs when the command returns

#### Scenario: A coordinator stops a working child
- **WHEN** a coordinator calls `stop_agents` on a child whose turn has a tool process running
- **THEN** the tool process no longer runs when the call returns
- **AND** anything the turn started after that is stopped when the turn ends

#### Scenario: A tool started a moment before the stop
- **WHEN** the human pauses an agent whose turn started a tool process a moment ago, before the host had recorded it
- **THEN** that process no longer runs when the command returns

#### Scenario: A sighting on its way
- **WHEN** a coordinator stops a child whose turn had taken a tool process's sighting from the watcher but not yet passed it to the record
- **THEN** that process no longer runs when the call returns

#### Scenario: The turn's last sightings on their way
- **WHEN** a provider has finished on its own leaving a process running, its last sightings and its end have not reached the host yet, and the agent is stopped
- **THEN** that process no longer runs when the stop returns

#### Scenario: The turn was finishing as its agent was stopped
- **WHEN** an agent is stopped and its turn's own, normal end reaches the host afterwards, with a process the stop's look came too early for
- **THEN** that process is stopped when the end is handled, with the reason `stopped`
- **AND** no notice says the turn left it running

#### Scenario: Stopping an agent whose compaction runs
- **WHEN** an agent is stopped while its compaction runs, with one process an earlier turn left running and one the compaction's provider started
- **THEN** the earlier turn's process no longer runs when the stop returns, and the compaction's process still runs
- **AND** when the compaction ends with the agent still stopped, that process is stopped with the reason `stopped`

#### Scenario: Stopping a project
- **WHEN** the human stops a project while one of its agents has a tool process running in its turn
- **THEN** that process no longer runs when the command returns

#### Scenario: Stopping a project with an idle agent
- **WHEN** the human stops a project one of whose agents is between turns and left a process running in an earlier turn
- **THEN** that process no longer runs when the command returns
- **AND** its group is recorded as stopped with the reason `stopped`

#### Scenario: A tool forked just before the cancel
- **WHEN** a turn is cancelled just after its provider forked a tool into a session of its own, before the watcher had listed it
- **THEN** the tool is recorded for the agent and stopped when the cancelled turn ends

#### Scenario: A look that fails
- **WHEN** the look made while a provider's group is stopped fails
- **THEN** the group is continued, not left stopped
