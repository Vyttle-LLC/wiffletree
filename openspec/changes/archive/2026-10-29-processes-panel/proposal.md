## Why

What an agent starts keeps running between its turns until the agent is archived or stopped, or the app quits. The human cannot see what each agent has running, or stop one thing without stopping the agent. In the incident that prompted this, eight `yes` processes ran for 41 minutes and had to be found and killed from a terminal.

## What Changes

- **A Processes panel on the inspector rail**, beside Files, for every role. On an agent it lists that agent's process groups; on the coordinator it lists every unarchived agent of the project, grouped by agent, with "Nothing running." for agents without any. Each row is one process group: its programs, "N processes · started HH:MM · group G", how long it has run, its CPU as a percentage of one core (highlighted at a full core or more) and Stop. The agent view also has Stop all.
- **A count on the rail entry** of the processes the record holds for the panel's agents, from the snapshot; no count when nothing runs.
- **Listing from the host.** `Command::Processes` answers the panel's agents with each group the record believes is running. A group is listed with its processes only while a recorded member, by process id and start time, is still alive in it. A group that runs but cannot be confirmed is listed as such, without processes and without Stop. A group that no longer runs is forgotten.
- **Live values only while the panel is open.** The desktop asks once a second while the panel is open and never while it is closed. CPU is worked out in the desktop from each process's CPU time across two readings.
- **The human's stop.** `Command::StopProcesses` stops the named groups, or every group the panel lists, through `processes::stop_group`, records them as `stopped_by_human`, and queues the "Stopped N processes …" notice for each agent's next turn. It does not pause or stop the agent.
- **Not included:** processes the host has no record of, keep-alive lists and platforms other than macOS beyond what compiles today.

## Capabilities

### New Capabilities
- `processes-panel`: the listing, the rail's count, the live readings and the human's stop from the panel.

### Modified Capabilities
<!-- None. The confirmation rule and the stop path are unchanged; the new stop reason is specified in processes-panel. -->

## Impact

- `workspace-core`: `RunningProcesses` on the snapshot; `AgentProcesses`, `ProcessGroup`, `GroupProcess`; `Command::Processes`, `Command::StopProcesses`.
- `workspace-host`: `agent_processes/panel.rs` (listing, the human's stop, the counts, CPU time from `proc_pidinfo`); `StopReason::StoppedByHuman`; `service.rs` answers `Processes` as a read and signals a change only when it forgot a group.
- `workspace-desktop`: `Panel::Processes`, the rail badge, a new `processes_view.rs`.
- Mockup: `design/mockups/current/source.html`, from the study the human approved on 2026-10-13.
