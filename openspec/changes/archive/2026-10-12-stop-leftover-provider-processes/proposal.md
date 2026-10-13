## Why

On October 12, 2026 the app was quit with an agent mid-turn and reopened. The agent's provider process kept running with permission prompts bypassed and nobody supervising it, and the reopened app started a second process for the same agent in the same worktree. Both resumed one provider conversation, which forked, and the agent lost one branch of it.

The system log shows a normal quit: the old app exited with status 0. A normal quit never stopped turns. The desktop app exits from `[NSApp terminate:]` without dropping the host, so the host's shutdown never ran; where it did run (the headless host), it recorded each turn finished without waiting for its process and the process exited first. Startup then marked the run interrupted without looking for its process, and nothing kept the session's next turn from starting beside it.

## What Changes

- **A quit stops running turns.** The host has a blocking shutdown that cancels every turn and returns once each provider's process group is stopped. The desktop app calls it when it quits, and the headless host when its input closes. A run is recorded finished only after its group is stopped; one still running at the deadline stays unfinished for the next start.
- **Startup stops what the last host left running.** Each run records its leader's start time beside its process group. For every unfinished run whose group still runs, startup stops the group as a turn's end does, but only while the leader's pid and start time are the recorded ones. Otherwise it signals nothing, raises one inbox note naming the session and the process id, and holds the session.
- **No turn beside a leftover process.** A held session starts no turn while that group runs. The hold ends when the group exits, or when the human dismisses the note because the process is not the agent's.
- **The retry note says so.** An agent retrying an interrupted turn is told when its earlier process was stopped at startup, since that process may have changed the worktree after the turn's last recorded step.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `workspace-host`: stopping turns at a quit, stopping leftover provider processes at startup, holding a session beside one, and the retry note.

## Impact

- `crates/workspace-host/src/provider.rs`: the leader's start time, and stopping a leftover group.
- `crates/workspace-host/src/service.rs`: shutdown, startup recovery, the hold and the retry note.
- `crates/workspace-host/src/worktrees.rs`: the `provider_runs.process_started` column.
- `crates/workspace-desktop/src/main.rs`, `bridge.rs`: shutting the host down when the app quits.
- Tests: `provider.rs`, `service.rs`, `tests/leftover_processes.rs`.
- One added column; existing runs keep `NULL` and are never signalled. No UI beyond the inbox note.
