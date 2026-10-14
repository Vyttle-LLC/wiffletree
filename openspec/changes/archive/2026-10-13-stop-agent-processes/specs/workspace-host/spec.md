## ADDED Requirements

### Requirement: The host records every process group an agent starts
While a provider run is live, the host SHALL watch the provider's process and its descendants on a clock that does not depend on the provider's output, sleeping no more than 5 ms between two looks, and listing the process table once per look however many runs are live. It SHALL record against the run's agent every process group seen among them, with each member's process id and start time, and SHALL add as members the processes it later finds in a recorded group while a recorded member is still in it. The record SHALL belong to the agent across its turns, SHALL hold any number of groups and SHALL survive a host restart. A group whose every member has exited SHALL be dropped from the record. A process seen in a group SHALL be added to the agent's open record of that group id only while a member that record holds is still alive in the group, by the rule that confirms a group before it is signalled; otherwise that group emptied unseen and its id was given out again, so the old record SHALL be dropped and a new one opened for the run that saw the process, with no notice yet. What the watcher saw SHALL keep reaching the record for as long as the provider runs, also after the provider has closed its output. When the watcher could not look for longer than a second, a process id may have been given out again in the meantime: a recorded group whose last known member has gone but whose id still names a running group SHALL then stay recorded as running, with none of that group's processes added as members, so the host later stops it or asks the human about it. A normal turn end SHALL stop the provider's own process group and SHALL leave every other recorded group running.

#### Scenario: A tool starts a process in its own session
- **WHEN** a turn's provider starts a child in a new session and process group, and the turn finishes normally
- **THEN** the child is still running
- **AND** its group is recorded against the agent with the child's process id and start time

#### Scenario: The group leader exits first
- **WHEN** the shell that leads a recorded group starts a child and exits before the turn ends
- **THEN** the group stays recorded with the child as a member

#### Scenario: The command finishes
- **WHEN** every process of a recorded group has exited
- **THEN** the group is no longer recorded

#### Scenario: A shell that lives between two looks
- **WHEN** a shell starts a child and exits without the watcher having listed the shell once
- **THEN** the child is not recorded

#### Scenario: The provider closes its output and keeps running
- **WHEN** a provider closes its output, then starts a process in a group of its own and keeps running
- **THEN** that group is recorded while the provider still runs

#### Scenario: The watcher could not look for over a second
- **WHEN** the watcher did not look for over a second, the last member it knew of a recorded group has exited meanwhile, and the group's id still names a running group
- **THEN** the group stays recorded as running and none of its processes is added as a member

#### Scenario: A group id given out again
- **WHEN** an agent's open record of a group holds only members that have exited, and the agent's current run is seen with a process in a new group that has the same id
- **THEN** the process is recorded in a new record of the current run, which no notice has named yet, and the old record is gone

#### Scenario: Several turns at once
- **WHEN** three providers run at the same time
- **THEN** the process table is listed once per look, not once per provider

### Requirement: A process group is signalled only while a recorded member is alive in it
The host SHALL signal a recorded process group only while at least one of its recorded members, matched by process id and start time, is alive and still in that group. It SHALL NOT require the group's leader to be alive and SHALL NOT signal on a group id alone. It SHALL send SIGTERM, wait the grace period, and send SIGKILL only if the group still runs and a process it confirmed is still alive in it. The same check SHALL be used when an agent is archived, when it is stopped, at a quit and at startup. A recorded group that no longer runs SHALL be dropped from the record with no signal. A group that still runs and cannot be confirmed SHALL receive no signal, SHALL stay recorded as running, and SHALL raise one inbox note on the agent that names the agent and the process group and says the host did not stop it. A group that still runs after the host signalled it, because processes joined it after the host confirmed it or outlast SIGKILL, SHALL NOT be signalled again while no process the host confirmed is alive in it, SHALL stay recorded as running, and SHALL raise the note saying that the host signalled the group and that part of it still runs, not that it did not stop it. No note SHALL be raised when nothing still runs. Each group the host stops SHALL be recorded with the time and one reason: `archived`, `archive_failed`, `stopped`, `quit` or `startup`.

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
When a session is archived, by any path, the host SHALL stop every process group recorded for that session across all its turns, under the confirmation rule, and SHALL do so before any ticket worktree is removed. The paths are the human archiving an agent or a project, `accept_ticket`, `close_ticket`, `archive_agent`, verification retiring a verifier and `verify_ticket` replacing one. A session archived while its turn still runs SHALL have its groups stopped when that turn ends. An archive that is refused because a worktree cannot be removed SHALL stop nothing. When an archive, a close or an accept fails after that check, what it stopped stays stopped: each such group SHALL be recorded with the reason `archive_failed` instead of `archived`, and each agent that is still unarchived SHALL receive the notice of what was stopped, saying that the archive then failed, with its next turn.

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

### Requirement: Stopping an agent stops what it started
When the human pauses a session, when the human stops a project, and when a coordinator calls `stop_agents`, the host SHALL stop every process group recorded for each agent it stops, under the confirmation rule, before the command or the tool call returns. It SHALL first look afresh at what each stopped agent's running turn has started, without waiting for that turn to pass it on, and SHALL record every process then followed for that turn, not only what is new since the turn last asked, so a process started a moment before the stop is stopped too, also when its sighting was on its way to the record. The watcher SHALL keep answering for a turn until the host has stored everything that turn sent, its last sightings included, so the look names what the turn started also after the provider has exited and while the turn's end is on its way. A turn that ends on its own as its agent is stopped SHALL be treated as a stopped turn: what it started SHALL be stopped and SHALL NOT be announced as left running. A stop lets a compaction that has started finish, so it SHALL leave alone what that compaction's provider started, such as a Codex app-server's tool servers; when the compaction ends and its agent is still paused or its project still stopped, what the compaction left running SHALL be stopped then. Stopping a project SHALL do so for every agent of the project, idle ones included: an emergency stop leaves nothing running unsupervised. When a cancelled turn ends, the host SHALL stop the groups recorded for its agent since. Each group SHALL be recorded as stopped with the reason `stopped`.

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

### Requirement: A turn that leaves processes running says so
When a turn ends normally and a group recorded for its agent still has live members that no notice has named, the host SHALL add one notice from Wiffletree to the agent's conversation that first states the number of processes, then names their programs, then each process group and when it started, and says they keep running until the agent is archived. Every notice SHALL name a program as a program, as in "All 8 are the program `yes`." or "2 are the program `node` and 1 is `esbuild`.", and SHALL NOT put a program's name beside a bare count. Before deciding, it SHALL wait up to 300 ms for processes that are exiting with the provider. The notice SHALL be delivered to the agent as input from Wiffletree with its next turn and SHALL NOT start a turn. A group SHALL be named in at most one such notice. When the host stops an agent's process groups, it SHALL add one notice from Wiffletree to that agent's conversation that first states the number of processes stopped, then names their programs the same way, then the groups and why. Neither notice SHALL raise an inbox item.

#### Scenario: Once per set of processes
- **WHEN** a turn leaves a process running and the agent's next two turns leave nothing new
- **THEN** the conversation has one notice naming that process, and the inbox has no item for it

#### Scenario: A later set of processes
- **WHEN** a later turn of the same agent leaves another process running
- **THEN** the conversation gains a second notice, which names only the new process group

#### Scenario: Eight of one program
- **WHEN** a Tester's turn leaves eight processes of the program `yes` running
- **THEN** the notice reads "Tester left 8 processes running. All 8 are the program `yes`."

#### Scenario: Several programs
- **WHEN** a turn leaves two `node` processes and one `esbuild` process running
- **THEN** the notice says "2 are the program `node` and 1 is `esbuild`."

#### Scenario: The agent is told
- **WHEN** the agent's next turn starts
- **THEN** its input includes the notice, from Wiffletree

#### Scenario: Stopped on archive
- **WHEN** the agent is archived and the host stops that process
- **THEN** the agent's conversation ends with a notice naming the process, its group and that it was stopped when the agent was archived

### Requirement: Agents are told how to stop what they start
The communication contract given to every agent SHALL say that a process the agent starts keeps running after its command ends until the agent is archived, that the agent stops what it does not mean to keep, that it saves each process id from `$!` and kills those ids, that it does not use `jobs -p`, which is empty in a zsh subshell, and that it afterwards confirms by name that nothing unintended is still running.

#### Scenario: Every role
- **WHEN** any agent's turn starts
- **THEN** its instructions include that rule

## MODIFIED Requirements

### Requirement: Compaction outcome comes from the provider's status record
The host SHALL decide a Claude agent's compaction's outcome from the status record Claude streams, not from the process's exit code or the result record's error flag. A `compact_result` of `success` SHALL be **compacted**, with the token counts before and after taken from the compaction boundary record when it is present. A `compact_result` of `failed` whose error starts with "Not enough messages to compact" SHALL be **nothing to compact**. Every other ending, including a missing status record, a nonzero exit and a process that could not start, SHALL be **failed**, with the provider's own reason when it gave one. When a process ended without a status record, the reason SHALL be what the run ended with held to one line of at most 200 characters, ended with "…" when it was cut, and the run SHALL keep the whole of it. A Codex agent's compaction SHALL be decided from its turn's completion notification, as its own requirement says. For either provider the host SHALL store the last outcome and its reason on the session's runtime, clear the request, and clear the stored outcome when a `DismissCompaction` command or a new request arrives. A compaction that is running when the host stops, by a shutdown or a crash, SHALL be settled as **interrupted**, at shutdown or at the next startup, through the handling every run gets. A shutdown SHALL cancel a running compaction and wait for its provider process group to be stopped as it does for a running turn, and SHALL record the run as finished only once the group is stopped; a compaction still running when the wait ends SHALL stay unfinished for the next start. At startup an unfinished compaction run SHALL be handled like any unfinished run, under the rule that confirms every recorded group before it is signalled: the host SHALL stop its process group only while a member it recorded for that group, matched by process id and start time, is still alive in it, whether or not that member is the group's leader, and SHALL otherwise signal nothing and raise the leftover-process note, which holds the session's turns and compactions until the group exits or the note is dismissed. An interrupted compaction SHALL leave the session's stored status and its waiting messages as they were, SHALL hold no input and SHALL need no resume. It SHALL NOT be run again by the host and SHALL NOT be reported as compacted or as unchanged, and the host SHALL NOT tell the session's next turn that a turn was interrupted.

#### Scenario: Success
- **WHEN** the stream has `"compact_result":"success"` and a boundary record with `pre_tokens` 812000 and `post_tokens` 5000
- **THEN** the outcome is compacted with those two figures and the request is cleared

#### Scenario: Nothing to compact
- **WHEN** the stream has `"compact_result":"failed"` with `"compact_error":"Not enough messages to compact."`, exit code 0 and `is_error` false
- **THEN** the outcome is nothing to compact, with that text as its reason

#### Scenario: Failure with exit code 0
- **WHEN** the stream has `"compact_result":"failed"` with another error and the process exits 0
- **THEN** the outcome is failed, with that error as its reason

#### Scenario: No status record
- **WHEN** the process exits without streaming a `compact_result`
- **THEN** the outcome is failed

#### Scenario: Quit while compacting
- **WHEN** the host shuts down while an agent's compaction is running and the agent has a message waiting
- **THEN** the compaction's process group no longer runs when the shutdown returns, the outcome is interrupted, the request is cleared, and the agent's status is what it was
- **AND** when the host starts again no compaction runs, and the agent's next turn takes the waiting message with a prompt that names no interrupted turn

#### Scenario: Crash while compacting
- **WHEN** the host starts with an unfinished compaction run whose provider process still runs with its recorded start time, and the agent has a message waiting
- **THEN** the process group is stopped, the compaction is settled as interrupted and is not run again, and the agent's status is what it was
- **AND** the agent's next turn takes the waiting message

#### Scenario: Crash while compacting, process not confirmed
- **WHEN** the host starts with an unfinished compaction run whose process id now belongs to a process with a different start time, and the agent has a message waiting
- **THEN** that process receives no signal, the compaction is settled as interrupted, and one inbox note names the agent and the process id
- **AND** no turn and no compaction starts for the agent while that process group runs, and its turn starts once the note is dismissed

### Requirement: A quit stops running turns
The host SHALL offer a shutdown that cancels every running turn, looks afresh at what each running turn has started, stops every process group recorded for any agent under the confirmation rule, and returns only once each of those turns' provider process groups has been stopped, as at any turn's end, or a bounded wait has passed. The desktop app SHALL call it before it exits from a quit, and the headless host SHALL call it when its input closes. A run SHALL be recorded as finished only after its process group has been stopped; a turn still running when the wait ends SHALL stay unfinished, and a group still running then SHALL stay recorded as running, so the next start handles both. Each group the shutdown stops SHALL be recorded with the reason `quit`.

#### Scenario: Quit during a turn
- **WHEN** the host is shut down while a provider is mid-turn
- **THEN** the provider's process group no longer runs when the shutdown returns
- **AND** the run is finished with the quit as its error and its input is held for a retry

#### Scenario: Quit with no turn running
- **WHEN** the host is shut down while no turn runs and no agent has a process recorded
- **THEN** the shutdown returns without waiting

#### Scenario: Quit while an agent's process runs
- **WHEN** the host is shut down while an agent between turns has a process it started still running
- **THEN** that process no longer runs when the shutdown returns
- **AND** its group is recorded as stopped with the reason `quit`

### Requirement: Startup stops processes the last host left running
At startup, before any turn starts, the host SHALL stop every process group still recorded as running for any agent, whether its run was left unfinished or its agent was between turns, under the confirmation rule, and SHALL record each group it stops with the reason `startup`. It SHALL then mark each run the last host left unfinished as interrupted and record on the run when its provider's process group was stopped, also when part of that group still runs after the host signalled it: the provider itself was stopped, the session stays held, and the note says the group was signalled. A run whose provider's group was neither stopped nor signalled SHALL record no such time, also when a process its agent had started was stopped at startup. A run an earlier version recorded with only a process group and its leader's start time SHALL be carried into the record when the store is opened, with that leader as its one member, or with no member when no start time was kept.

#### Scenario: Leftover provider
- **WHEN** the host starts and an unfinished run's provider process is still running with its recorded start time
- **THEN** the process group is stopped before any turn starts
- **AND** the run is interrupted and records when its process was stopped

#### Scenario: Killed mid-turn with a tool process running
- **WHEN** the last host was killed mid-turn while a process the agent started was running
- **THEN** startup stops that process

#### Scenario: The provider had gone and a tool had not
- **WHEN** the last host was killed mid-turn, the run's provider has since exited and a process the agent started still runs
- **THEN** startup stops that process
- **AND** the run records no stop time, and the agent's retry note does not say its provider kept running

#### Scenario: Killed between turns
- **WHEN** the last host was killed while an agent between turns had a process it started still running
- **THEN** startup stops that process

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

#### Scenario: The provider exits and a group member ignores the signal
- **WHEN** the host starts, signals a confirmed leftover, its leader exits on SIGTERM and another member of the group ignores it
- **THEN** that member is killed, the run is interrupted and records when its process was stopped, and no note is raised

#### Scenario: Part of a stopped provider's group survives
- **WHEN** the host starts, signals a confirmed leftover, every process it confirmed exits and a process that joined the group after that keeps running
- **THEN** the run is interrupted and records when its process was stopped
- **AND** one inbox note says the group was signalled and that part of it still runs
- **AND** the session starts no turn while that group runs

### Requirement: No turn starts beside a leftover process
While the provider process group of one of a session's finished runs is still recorded as running and still runs, the host SHALL start no turn for that session. It SHALL hold the turn as other held turns are, with the reason naming the process id, and SHALL check again without waiting for another event. When the group no longer runs, the host SHALL settle its note itself and start the turn. The host SHALL settle a leftover-process note once it sees its group gone whether or not the session has a turn due, and SHALL look again without waiting for another event while a note is open, so no note keeps saying a process runs after it exited. When the human dismisses the note, the hold SHALL end and the group SHALL no longer be recorded as running. A run an earlier version finished without its process having been seen to end, marked interrupted or finished by a quit, SHALL be carried into the record when the store is opened, so it holds its session the same way; once its group no longer runs it SHALL be dropped and never looked at again, since process ids are reused. Only a provider's own process group SHALL hold a session: a process group the agent started with its tools SHALL never hold the agent's turns, whether or not the host could confirm it.

#### Scenario: Retry while the process runs
- **WHEN** an interrupted session is retried while the provider process group recorded for its interrupted run still runs
- **THEN** no turn starts and the session shows as blocked with a reason naming the process id

#### Scenario: The process exits
- **WHEN** that process group exits
- **THEN** the host settles the note and starts the session's turn

#### Scenario: Run an earlier version marked interrupted
- **WHEN** a session has a finished, interrupted run with no note whose process group still runs, and input is queued for it
- **THEN** no turn starts, one inbox note names the session and the process id, and the turn starts once the group exits

#### Scenario: Process id reused later
- **WHEN** a run's process id is given to another process after its group was dropped from the record
- **THEN** the session is not held and no note is raised

#### Scenario: The human dismisses the note
- **WHEN** the human dismisses the note because the process is not the agent's
- **THEN** the session's turn starts

#### Scenario: The process exits while no turn is due
- **WHEN** the process group a note names exits while its session is disconnected with its input held
- **THEN** the host settles the note without the session being retried

#### Scenario: The agent's own process keeps running
- **WHEN** an agent's turn left a process running and a message arrives for the agent
- **THEN** its next turn starts while that process runs

## RENAMED Requirements

- FROM: `### Requirement: Startup stops provider processes the last host left running`
- TO: `### Requirement: Startup stops processes the last host left running`
