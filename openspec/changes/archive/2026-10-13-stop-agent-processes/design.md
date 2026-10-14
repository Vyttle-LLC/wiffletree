## Context

A provider runs as the leader of its own process group. `Process::stop` stops that group at a turn's end and is safe because the unreaped leader keeps its id from being reused. Each run stored that one group and its leader's start time in `provider_runs`, and startup stopped it only while the leader was still the recorded process.

That covers the provider and the helpers that stay in its group. It does not cover what the agent runs. Measured on October 13, 2026 (macOS 27.0.1, Claude Code 2.1.296, Codex 0.160.0):

- **Claude Code** runs each Bash call as `zsh -c …`, the leader of its own session and process group. This implementer's own turn: provider 11906 in group 11906, its tool shell 51723 in group 51723.
- **Codex** does the same. One `codex exec` turn that ran `sleep 12`: the command was process 57511, the leader of session and group 57511, while the provider was in another group. Codex also starts each of its tool-server helpers in a group of its own; they exit when the provider does.

So for both providers nothing a tool starts is in the provider's group, and the tracking below treats the two the same: it follows descendants, whatever group they are in.

## Findings the design rests on

**macOS never hands out a process id that is still a live process group id.** Tested: a group whose leader had exited and been reaped, with one member left, kept its id through 99,151 forks, a full lap of the id counter (highest id 99,998). After that member was killed, the next lap handed the id out. The kernel source agrees: `forkproc` in `bsd/kern/kern_fork.c` skips an id while `phash_find_locked(pid)`, `pghash_exists_locked(pid)` or `session_find_locked(pid)` finds it. So while one process of a group is alive, the group's id cannot become another process's id, and since a process can only join a group of its own session, every process in that group belongs to it.

What can be reused is a group id whose group has emptied. A stored group id alone therefore proves nothing; a stored member that is still alive in the group proves the group never emptied.

**A tool shell is short-lived.** `zsh -c` with a real Claude Code snapshot file, starting one background child and exiting, lived 9.9 to 11.4 ms over 30 runs. A bare `/bin/sh -c 'sleep 5 &'` lived about 3 ms. The brief expected tens of milliseconds; on this Mac it is ten.

**Reading the process table is cheap when warm and dearer after a sleep.** In a tight loop `proc_listallpids` took 5.4 µs for 592 processes, `proc_pidinfo` 0.5 µs per process and `proc_listpgrppids` 5.1 µs, and one whole scan of the finished watcher 12.5 µs. Called once after each 4 ms sleep, the same listing costs about 80 µs of CPU, because walking the kernel's process list then misses the cache; a bare 4 ms sleep costs 6 µs per wake.

**An orphan keeps its group and loses its parent.** When a tool shell exits, its children are handed to `launchd` at once. They are no longer the provider's descendants, but they keep the shell's group id, and `proc_listpgrppids` still lists them.

## Decisions

**One watcher for every running turn, on a 4 ms clock.** A process-wide watcher thread runs while any provider run is live. Each tick lists every process id once, reads only the ids it has not listed before, and adopts a new process into a run's tree when its parent is already in the tree or its group is one the tree holds. It then reads each process in each tree again, because a child often changes group just after it is forked. When the last known member of a group has gone, it lists the group once: members it finds are adopted, since the group never emptied, and otherwise the group is reported gone. The cost of a tick is one listing plus one read per new process and per tracked process, whatever the number of running turns, and a tree is capped at 4,096 processes. Its thread starts with the first provider run and exits when the last one ends, so it runs only while at least one run is live.

Measured CPU cost of the finished watcher on this machine (an Apple M4 Mac with 10 cores, macOS 27.0.1, 592 to 675 processes, release build), over five seconds each, about 200 scans a second since the 4 ms sleep comes to 5 ms between scans:

| Running turns | Processes followed | CPU per scan | CPU per second | Share of one core |
| --- | --- | --- | --- | --- |
| 0 | 0 | the thread is not running | 0 ms | 0% |
| 1 | 5 | 91 to 97 µs | 18 to 19 ms | 1.8% to 1.9% |
| 8 | 40 | 94 to 135 µs | 19 to 27 ms | 1.9% to 2.7% |

Two runs of each; the spread between runs is as large as the difference between one turn and eight. The verifiers of this change measured more on the same machine with more processes running: 3.0% to 3.1% with 807 processes, and 2.2% to 4.2% with about 770. So the cost is roughly 2% to 4% of one core in a release build, rising with the number of processes on the machine, since each scan lists them all, and not with the number of turns. A debug build measured 18% to 20%. Against the PRD's budgets: the steady-idle budget of 1% of one core is untouched, because the watcher does not run between turns; while turns run, the watcher takes a fifth to two fifths of the 10% the PRD allows the app while ingesting 1,000 events a second, and that budget excludes provider execution, which is far larger. A turn's own thread only collects what the watcher saw, each time its loop wakes, and sends it to the host before the turn's end.

The interval is 4 ms because a tool shell lives about 10 ms: the watcher must list a shell once while it lives, and from then on its group is followed even after the shell exits. The 50 ms wake of the provider's read loop would miss most shells that start a background process and return at once, which is the commonest way an agent leaves something running.

**The record belongs to the agent.** Two tables, plain to query:

```sql
CREATE TABLE process_groups (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,       -- the agent
  run_id TEXT NOT NULL,           -- the run that first saw the group
  process_group INTEGER NOT NULL,
  provider INTEGER NOT NULL,      -- 1 for the run's own provider group
  seen_at INTEGER NOT NULL,
  noticed_at INTEGER,             -- when a conversation notice named it
  ended_at INTEGER,               -- when the host stopped it, or the note was dismissed
  ended TEXT                      -- archived | archive_failed | stopped | quit | startup | dismissed
);
CREATE TABLE process_group_members (
  group_id TEXT NOT NULL REFERENCES process_groups(id) ON DELETE CASCADE,
  pid INTEGER NOT NULL,
  started INTEGER NOT NULL,       -- microseconds since the epoch
  command TEXT NOT NULL,
  PRIMARY KEY (group_id, pid, started)
);
```

A row with no `ended_at` is a group the host believes is running; an agent has at most one such row per group id. A group that exits on its own, and a provider's group stopped at its turn's end, are deleted, so the table holds what runs now and what the host stopped, when and why. While a run is live a group's members only gather, exited ones included; when the turn ends, and when a group is stopped, its members are replaced by the processes in it at that moment, so between turns the record is exact. `provider_runs.process_group` and `process_started` are no longer written. Opening a store copies each old run that could still have a live process (unfinished, or finished without its process having been seen to end) into the record under the run's own id, with the leader as its one member, or none when no start time was kept. The id matters: an inbox note an older version raised is `leftover-process:<run>`, and the note of a record is `leftover-process:<record id>`, so old notes keep meaning what they meant.

**One confirmation rule, used everywhere.** A group is signalled only while at least one recorded member, with the recorded process id and start time, is alive and still in that group. Then every process now in the group is taken as confirmed, SIGTERM is sent, and after the existing one-second grace SIGKILL is sent only if a confirmed process is still alive in the group. The leader is not required. If the group no longer runs, nothing is signalled and the row is deleted. If it runs and nothing can be confirmed, nothing is signalled, the row stays, and the agent gets one inbox note. Archive, stop, quit and startup all call this one function.

**When the host stops an agent's groups.**

| Occasion | Which agents | Reason recorded |
| --- | --- | --- |
| A session is archived, by `Host::change_archived` (the human's archive, `archive_agent`, a retired or replaced verifier) or `finish_ticket` (`accept_ticket`, `close_ticket`) | Every session being archived that has no unfinished run | `archived` |
| An archived session's turn ends | That session | `archived` |
| The human's Pause, a coordinator's `stop_agents`, the human's project Stop | The agents stopped, at once and before the command or tool returns | `stopped` |
| A cancelled turn ends | That session, for groups recorded since the stop | `stopped` |
| A quit | Every agent, then each cancelled turn's agent again as its turn ends | `quit` |
| Startup | Every agent with a group still recorded as running | `startup` |

Archiving and `finish_ticket` check that the worktrees can be removed, then stop the processes, then remove the worktrees, so a refused archive stops nothing and no worktree is removed under a live process. Groups are stopped together, so the wait is one grace period however many there are. The stops run on the host thread: a process that honours SIGTERM costs a few milliseconds, one that ignores it up to two seconds.

**A turn's own tool shell is not stopped by an archive.** A verifier is archived while its turn is still writing its summary. Its groups are stopped when that turn ends, not while it runs.

**Notices are messages from Wiffletree.** Each states the count first and then names the programs as programs. The notice a turn leaves is a quiet message to the agent under a `processes:` id: it shows in the agent's conversation as a host notice, rides along with the agent's next turn as input from Wiffletree, and never starts a turn. `noticed_at` keeps it to once per group. Before a normal turn's end is reported, the provider's thread waits up to 300 ms for the tree's other processes to exit, so helpers that are shutting down with the provider are not announced. A stop notice is the same kind of message; for an archived agent it is stored as already complete, since no turn will take it. Provider groups are never named in a notice: the retry note already covers a provider stopped at startup.

**Only a provider's group holds a session.** The hold exists so that two provider processes never run on one conversation. It now reads the record: a session starts no turn while a provider group of one of its finished runs is still recorded as running and still runs. A group the agent started with its tools never holds its next turn, confirmed or not, and its inbox note does not say it does. Dismissing a note marks its record `dismissed`, which also frees the worktree.

**Worktree removal reads the record.** An agent counts as in its turn while it has an unfinished run or any recorded group that still exists. Since archiving stops the groups first, this only defers removal for a group that could not be confirmed.

**Other choices considered.** Tagging the provider's environment with the run id was ruled out by the human's own test: this macOS does not return another process's environment. Scanning from the provider's read loop every 50 ms is simpler but would list a 10 ms shell only about one time in five. A kernel queue with `NOTE_FORK` on every tracked process would shrink the gap below to well under a millisecond; it is the natural follow-up if the gap turns out to matter, and was left out because the brief asks for a clock. Matching processes by working directory or command line is a guess, and the rule is to signal nothing on a guess.

## Known gaps

- **A shell the watcher never lists.** A shell that starts and exits between two ticks, under about 4 ms plus scheduling delay, is never seen, and what it leaves behind is not recorded. A test pins this with a watcher ticked by hand.
- **A process that daemonizes after its parent has exited.** It is in a new session and group and no longer anyone's descendant, so it is not recorded.
- **A tool forked just before a cancel.** A tool the provider forks within one scan interval before a cancel kills the provider is orphaned before any scan lists it, so it is never recorded. A later change could close this by freezing the provider's group before the final look.
- **Sightings travel with the turn.** What the watcher saw reaches the store when the turn's thread next wakes, at most 50 ms later, for as long as the provider runs. A host that dies in between has not recorded it. A stop or a quit does not wait: it asks the watcher, which answers for a run until the host has stored the run's last sightings.
- **A watcher that could not look for over a second.** A process id may have been given out again in that time, so a group whose last known member has gone is neither adopted nor dropped while its id still names a running group. Its record stays, and a later stop either finds it gone or raises the inbox note.
- **Members are refreshed during the run that saw the group.** A group that later replaces every one of its processes while its agent is idle can no longer be confirmed; it raises the inbox note instead of being stopped.

## Risks

- The watcher costs roughly 2% to 4% of one core in a release build while any turn runs, more the more processes the machine runs, nearly all of it the cold listing after each sleep. It does not run when no turn runs. Being told of forks by the kernel instead of looking on a clock would remove that cost; see the other choices considered.
- A quit or an archive waits out the grace period when a process ignores SIGTERM.
- A helper that takes longer than 300 ms to exit after its provider is announced as left running, once.

## Rebased onto review levels and Codex compaction

**Codex compaction (#56).** A Codex agent's compaction runs `codex app-server`, and it runs through the same `provider::run` as every turn and as Claude's compaction: the same spawn in a group of its own, the same watch under the run's id, the same sightings and the same end. So the rules hold for it without a path of its own. The server's own group is stopped by its run when the outcome is read or its time is up. What it starts in other groups, which for Codex means its tool servers, is followed and recorded against the agent, and the run's tree is released when the host has handled the run's end. Three things are specific to a compaction:

- A stop lets a started compaction finish, an earlier requirement. So Pause, the project's Stop and `stop_agents` leave alone what the compaction's run started, and stop only what the agent's earlier runs left. When the compaction ends, what it left running is stopped if its agent is still paused or its project still stopped, and announced as left running otherwise, as after a turn.
- An archive never reaches into a running compaction: an agent with an unfinished run keeps its processes until the run ends, and an archived agent's run then stops them.
- A quit cancels the compaction and stops everything, the look of its own included.

A run now keeps its provider's process group id in its own detail (`$.process_group`), as a fact about the run that outlasts the record's row, since a finished run's provider row is deleted. Nothing is signalled by it.

**Review levels (#58).** Nothing new archives a session. A full review and a re-check both start their verifiers through the one loop that reuses a verifier at its profile or archives it with `change_archived` (`verifier_replaced`), and every end of a cycle, the new one where a ready report ends a cycle included, retires verifiers through `retire`, which calls `set_archived`. All of them therefore stop what the verifier started. Replacement, which had no test of its own, now has one.

## Why a stop's look names every live process

Sightings travel from the watcher to the store through the turn's thread and the host's queue, and a stop is handled at some moment of its own. Each fix so far closed one moment and left the next. The ownership is now the other way round: the watcher keeps a run's tree, and `followed_now` answers for it, until the host has handled the run's end and calls `processes::release`. The thread's `Watch` no longer removes the tree; dropping it only hands the tree over. A stop (Pause, the project's Stop, `stop_agents`, a quit) looks while the host still has the turn, records everything the look names and then stops what is recorded. Point by point, for a process P the agent's turn started:

1. **The watcher follows P; the thread has not taken its sighting.** P is in the tree. The look names it.
2. **The thread has taken the sighting and not sent it, or it waits in the host's queue behind the stop.** Taking a sighting does not take P out of the tree. The look names it.
3. **The provider has exited on its own; the thread waits for what is left to settle, or has made its last look and holds the last sightings.** The tree is still the watcher's. The look names P.
4. **The last sightings and the turn's end are in the queue behind the stop.** The thread is gone and its `Watch` dropped, which only marks the tree handed over. The look names P.
5. **The host has handled the turn's end.** The thread sent every sighting before the end, on the one queue the host reads in order, so the store has them all. Only now is the tree released, and the stop reads the store, as for any agent between turns.
6. **P is alive and the watcher has not looked since it appeared.** The look scans first and adopts P if its parent or its group is in the tree.
7. **P is still in the provider's own group, between its fork and its move to a session of its own.** The look leaves the provider's group to the turn, which stops it. If P leaves the group first it is not named. The stop has set the turn's cancel flag, the thread's last look sees P, and the host stops what the agent started when that turn ends. This is the one case where P outlives the stop's return, by the time the provider's loop takes to see the flag and stop its group: at most 50 ms, plus the one-second grace if the provider ignores SIGTERM.

**A turn that is finishing normally at that instant.** If the stop is handled before the turn's end, points 3 and 4 apply and P is stopped before the stop returns. When the end then arrives it says nothing of a stop, but the turn's cancel flag is set, so the host treats it as a stopped turn's end: it stops whatever was recorded since and announces nothing as left running. If the stop is handled after the end, the agent is between turns, the store is complete and P is stopped from there.

A handed-over tree is dropped by the watcher itself once nothing in it is alive, so a run whose end never reaches the host costs nothing once its processes are gone. The gap that remains is the documented one: a process the watcher never saw is in neither place.

## Fixed in review, cycle 2

- A stop handled after the turn's thread had finished, but before its last sightings were stored, found no tree and no record and returned with the tool alive. The watcher now keeps the tree until the host releases the run; see above.
- A turn that ended on its own while its agent was being stopped was settled as a normal end, so what it had started could be announced as left running. The turn's cancel flag now decides.
- Two test defects: the in-transit test shared the run id `run` with other tests' turns in the process-wide watcher and had its process stopped by them; and helpers read a stopped process's exit status before it could be collected. The test now has a run id of its own, and the helpers wait for a process the record says was stopped.

## Fixed in review, round 2

- The look a stop takes returned only what was new to the watcher. A sighting the turn's thread had already taken, but the host had not yet stored, was in neither place, and the stop returned with the tool alive. `processes::followed_now` now returns every process followed for the run, found by the run's id, so the look needs nothing the store may not have yet. The archive of an agent in its turn has no such gap: its processes are stopped when the turn ends, and the turn's thread hands on everything the watcher saw before it reports the end, on the one channel the host reads in order.
- Startup put a stop time on a run when any group of the run was stopped, so a run whose provider had gone and whose tool was stopped told its agent that the provider had kept running. Only the provider's own group counts now.
- An archive, close or accept stops the agents' processes before it removes worktrees, outside that change. When the change then failed, the agents were still there with their processes gone, the record said `archived` and the notice was stored as already read. The notice and the reason are now written once the outcome is known: on a failure the record says `archive_failed` and the notice waits for the agent's next turn. One case is not covered: an archive nested in a larger change that later fails, as when `verify_ticket` replaces a verifier and then cannot start the cycle, rolls the record and the notice back with it.
- The scripted provider's lingering `sleep` is killed by its saved id; a second set of processes left by a later turn has a test for its own notice.

## Fixed in review, round 1

- A stop returned before a tool started a moment earlier was stopped, because the tool's sighting was still with the turn's thread. `stop_processes_of` and the quit now call `Host::record_tree_now`, which has the watcher look at once and records what it sees, before they stop anything.
- After a pause of over a second the watcher reported a group as exited without probing it, and the host dropped its record though an orphan still ran. It now reports an exit only when no process has the group's id.
- A provider that closed its output and kept running had what it started recorded only when it exited. The loop that waits for its exit now passes sightings on as well.
- The integration tests waited for a run's `finished_at`, which the host writes first, and then read what it writes last. They now wait for the turn's `turn_finished` entry.

## Rebased onto the October follow-ups

The follow-ups change (#54) landed first. It gave startup a third outcome, a leftover it signalled whose members survived, and settled leftover notes on every scheduler pass. Both are kept on the record:

- `stop_group` returns `Signalled` when it signalled a group that still runs. The record stays, the note says the group was signalled and that part of it still runs, a provider's run records when it was stopped, and its session stays held.
- Under this change's confirmation rule that outcome is rarer than it was. A member that ignores SIGTERM beside a leader that exits on it was in the group when the host confirmed it, so it is killed. Only a process that joins after the confirmation, or one that outlasts SIGKILL, is left.
- Each scheduler pass settles every open leftover-process note whose group is gone, for providers' groups and agents' own alike, with or without a turn due, and wakes again in two seconds while one is open.

## Decided after the first report

- The human's project **Stop** stops the recorded processes of every agent in the project, idle ones included, and Pause stops the paused session's: an emergency stop that left an idle agent's processes running would repeat the incident.
- The notices state the count first and name programs as programs, "Tester left 8 processes running. All 8 are the program `yes`.", never a name beside a bare count: the human could not read "`yes` × 8".
- A Processes panel is an approved follow-up. It will read this record, which keeps per group the member process ids with their start times, each member's program name, when the group was first seen and which run saw it first.
