## Context

A provider runs as the leader of its own process group, and each run stores that group's id. `Process::stop` stops a group with SIGTERM, a one-second grace and SIGKILL, and is safe because the unreaped leader keeps its pid from being reused. A leftover group has no such pin: its leader is no longer our child, so its id may by now belong to another process.

## Decisions

**The leader's start time identifies it.** The provider thread reads the leader's start time from the kernel right after spawning it (`proc_pidinfo`, microseconds) and stores it as `provider_runs.process_started`. A pid and its start time together are not reused. Startup signals a group only while its leader has the recorded pid and start time, and checks again before SIGKILL. Where the start time cannot be read (platforms other than macOS, or a run recorded before this change), identity is never confirmed, so nothing is signalled.

**Anything not confirmed is left alone and held.** A missing or different start time, or a group whose leader exited and left other members, all take the same path: no signal, one inbox note, session held. A different start time proves the id was reused, but the same path keeps one rule, and the human can dismiss the note to release the session.

**The open inbox note is the hold.** The note's id names the run, so the hold needs no other stored state and survives a restart. The scheduler holds the session through the existing held-turn path (blocked, reason shown, parent told once), checks the group again every two seconds, and settles the note itself when the group has exited.

**A run is finished only once its process is stopped.** Shutdown used to record every turn finished at once. It now waits, up to three seconds, for each provider thread to report the end of its turn, which the thread does after stopping and reaping its group. A turn still running then stays unfinished, so startup is the one place that deals with a process nobody stopped. This is what makes the hold complete: a finished run never has a live group, apart from the ones startup could not stop.

**Other choices considered.** Scanning every earlier run of a session for a live group would hold sessions on reused ids from old history. Matching a leader by its command line or working directory is weaker than its start time and no simpler.

## Risks

- A host that dies between spawning a provider and recording its group leaves a process no run names. The window is one event on the host thread.
- A process that left the provider's group (its own session) is outside the group and is not stopped, as at a turn's end today.
- Quitting waits for the grace period when a provider ignores SIGTERM, so a quit with agents working can take about a second.
