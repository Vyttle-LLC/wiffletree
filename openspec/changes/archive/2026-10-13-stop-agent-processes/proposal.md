## Why

On October 13, 2026 a Tester ran one Bash command that started eight `yes` processes. Its own cleanup failed silently, because `kill $(jobs -p)` is empty in a zsh subshell. Wiffletree recorded the turn as completed and never noticed them. They ran 41 minutes at about 85% of a core each until they were killed by hand.

The host only knows the provider's own process group. `Process::stop` signals that group at a turn's end, and the comment on `Process` says it holds "the provider and its tool subprocesses". That is false: Claude Code and Codex both run each tool command as the leader of its own session and process group. Startup recovery looks only at unfinished runs and only at that one recorded group, and `stop_leftover` refuses a group whose leader has exited, which is exactly what an orphan's group looks like.

## What Changes

The rule, decided by the human on October 13, 2026:

- **What an agent starts keeps running.** A dev server or any other process an agent starts keeps running after its command finishes and between its turns. A normal turn end stops only the provider's own process group, as today.
- **Archiving stops it.** When an agent is archived, by any path, the host stops everything that agent started, across all its turns, before any worktree is removed.
- **So do a stop and a quit.** Stopping an agent (the human's Stop or Pause, a coordinator's `stop_agents`, a cancelled turn) and quitting Wiffletree stop that agent's processes. Startup stops whatever an earlier host left running, for unfinished runs and for agents between turns alike.
- **Nobody is surprised.** A turn that leaves processes running adds one Wiffletree notice to the agent's conversation, and the agent reads the same notice at the start of its next turn. Stopping an agent's processes adds a notice saying what was stopped and why. Only a group that still runs and could not be confirmed goes to the inbox.

What makes that possible:

- **Track.** While a provider run is live, one shared watcher reads the process table after each 4 ms sleep and records every process group seen among the provider's descendants, with each member's process id and start time.
- **Persist per agent.** Two new tables, `process_groups` and `process_group_members`, replace `provider_runs.process_group` and `process_started`. A record belongs to the agent, holds any number of groups, and survives a restart.
- **Confirm before signalling.** A group is signalled only while at least one recorded member, matched by process id and start time, is alive in it. The leader need not be alive.
- **Tell agents.** The communication contract (version 8) tells every agent that what it starts keeps running until it is archived, and how to stop what it does not mean to keep.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `workspace-host`: recording the process groups an agent starts, the confirmation rule, stopping them at archive, stop, quit and startup, the notices, and which leftover process still holds a session.

## Impact

- `crates/workspace-host/src/processes.rs` (new): reading a process, the shared watcher, and stopping a group under the confirmation rule.
- `crates/workspace-host/src/agent_processes.rs`, `processes.sql` (new): the per-agent record, its migration from `provider_runs`, the stops, the notices and the inbox note.
- `crates/workspace-host/src/provider.rs`: a run watches its provider's descendants and reports what it sees; the comment on `Process` is corrected.
- `crates/workspace-host/src/service.rs`: recording sightings, the turn-end notice, stops on Stop, Pause, `stop_agents`, a cancelled turn, quit and startup, and the session hold.
- `crates/workspace-host/src/lib.rs`, `live.rs`, `worktrees.rs`: every archive path stops the agent's processes before a worktree is removed; worktree removal reads the new record.
- `crates/workspace-core/src/lib.rs`: the `processes:` notice prefix.
- `crates/workspace-host/skills/communication.md`: version 8.
- Tests: `processes.rs`, `service.rs`, `tests/leftover_processes.rs`, `tests/archive.rs`.
- Two added tables; the two `provider_runs` columns stay and are no longer written. No UI beyond the existing host notice and inbox note.
