## Context

The host records every process group an agent's tools start in `process_groups` and `process_group_members`, with each member's process id and start time, and stops them only through `processes::stop_group`, which signals a group only while a recorded member is still alive in it. The Files panel already sets the pattern for an inspector panel that asks the host only while it is open.

## Decisions

- **A request, not the snapshot, for the list.** Running time and CPU change every second; the snapshot is rebuilt on every state change and must not read the process table. The panel sends `Command::Processes` once a second while it is open. The service treats it as a read, so it triggers no snapshot reload, and signals a change only when the listing forgot a group that had exited, which changes the rail's count.
- **The count from the record.** The rail's count is one grouped count over the open tool groups' members, added to the snapshot. Between turns the record is exact; during a turn it can still count a member that exited inside a live group, and a process killed from a terminal between turns is counted until the panel is opened or the agent's next turn ends.
- **CPU in the desktop.** The host returns each live process's CPU time (`proc_pidinfo(PROC_PIDTASKINFO)`, Mach ticks converted to nanoseconds) and keeps no state. The desktop divides the time used by processes present in both of two readings by the wall time between them. The first reading shows "—".
- **Cost.** Per reading: one SQLite query, and per recorded group a `killpg(g, 0)` probe, one `proc_pidinfo` per recorded member, one `proc_listpgrppids` and two `proc_pidinfo` per live member. Nothing while the panel is closed.
- **The human's stop on the host thread.** As for archive, the stop runs `stop_group` for the groups together and can hold the host thread for up to about two seconds (SIGTERM, one second's grace, SIGKILL).
- **Scope of a stop.** `StopProcesses` names the panel's session, so a coordinator's row stops any of its project's agents' groups; with no groups named it stops every group the panel lists. The desktop offers Stop all only on one agent's panel.
- **Choices the mockup left open**, reported to the coordinator: a group with several programs names them most common first ("`node`, `esbuild`"); CPU is highlighted at 100% or more; Stop all sits in the panel's head; an unconfirmed group reads "Process group G" with why it will not be stopped; the head says "8 processes running" rather than "8 running".
