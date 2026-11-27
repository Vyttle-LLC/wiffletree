# The human's commands on a busy host

Measured November 27, 2026 on an Apple M3 Pro, 36 GiB memory, 12 logical CPUs, macOS 27.0.1,
Rust 1.100.0, release builds. These are development-machine samples, not a performance guarantee.
Other sessions' builds ran throughout, so the load average moved between 10 and 70 on top of the
load below. Before and after runs alternated so that this background affected both.

## Why it was measured

With the Mac busy, the Send button and Inbox cards sat on "sending". Those wait for the host's
reply, so this measures how long the host takes to answer the human's commands, and why.

## Load and method

- **Agents.** The many-agents fixture (`examples/many_agents.rs seed`; see
  [refresh-cost.md](refresh-cost.md)): 40 agents in five projects, kept working by
  `busy_provider.py`. Its coordinators also ask the human a question each turn, so the Inbox has
  questions to answer and dismiss.
- **CPU.** One `yes > /dev/null` per core, at normal priority.
- **Commands.** `many_agents latency <folder> [seconds]` waits 20 s, then gives one command every
  250 ms, in turn: Snapshot, Send to a coordinator, ResolveAttention and DismissAttention on open
  questions, SettlePausedFile and DismissPairedNotice. Each is timed from the request to its
  reply over `host.sock`. The two settle actions name a file and a device that do not exist, so
  they are refused, but they still wait in the actor's queue like real ones.

Two set-ups were measured:

- **Child host**, as development builds run it. The probe starts the host as the app does. This
  session ran at niceness 10, so the probe, host and burners all inherited it and competed at equal
  priority, as the app's child host and other apps do at niceness 0.
- **launchd host**, as the signed app runs it. The host, the burners and the probe each ran as a
  launchd job in the user's GUI domain, at niceness 0. The burners and the probe, which stands in
  for the app, were Interactive jobs. The host had no `ProcessType`, as before this change, or
  `Interactive`, as after it. The probe joined with the host's credential through
  `WIFFLETREE_CREDENTIAL`.

```sh
cargo build --locked --release -p workspace-host --bin wiffletree-host --example many_agents
target/release/examples/many_agents seed /tmp/latency/fixture   # a fresh copy for each run
for i in $(seq $(sysctl -n hw.ncpu)); do yes > /dev/null & done
WORKSPACE_CLAUDE_BIN=$PWD/crates/workspace-host/examples/busy_provider.py \
  target/release/examples/many_agents latency /tmp/latency/fixture 180
```

## What the commands waited behind

Every command from the app and from devices already went on the actor's commands lane, ahead of
queued agent output (`Requests::request`, `request_from` and `tell_first`). So a command waited
only for the event being handled when it arrived. Under load, those events took long:

- **OS scheduling.** Without the burners, every command answered within 17 ms at p95. With them,
  slow actor turns spent about 33 s of wall time but only 1.8 s of CPU in a 90 s run. The actor
  was mostly waiting for processes it ran, for disk writes and for the CPU.
- **A Git process per ticket in the actor.** A profile with `sample` put about 16% of the actor's
  busy time in `reply_update`. At each coordinator's turn end it ran `git rev-parse` once for each
  open ticket, which made those turns 200–400 ms long.
- **Brain index files opened at every state change.** About 20% of busy time was in `signal`, which
  counts the Inbox. The count includes conflict cards, so it opened every brain's index file,
  about 15 times a second.
- **launchd throttling.** A launchd job with no `ProcessType` gets throttled CPU and I/O. Against
  normal-priority work, the signed app's host answered in 0.1–1.8 s at p95 and up to 2.9 s.

The rest of the actor's time, which this change does not move, goes to launching providers,
step writes (each a SQLite commit with fsync), reading skill settings files and store queries. No
one of these dominates.

## Changes

- A coordinator's reply reads each open ticket's branch head from the repository's loose or
  packed ref, without a Git process. It falls back to Git only when it cannot read the ref
  directly: a linked worktree's `.git` file, a reftable store or a symbolic ref.
- A brain index's read connection stays open while the file's inode, modification time and
  length are unchanged, so counting the Inbox opens no file.
- The launchd job is `Interactive`: the app waits on each of its replies. Agents still run at
  `AGENT_NICENESS`.

Ordering is unchanged. The commands lane, the bounded events queue and each session's storage
order are as before.

## Before and after

Milliseconds as p50 / p95 / max, with the sample count. Each run lasted 180 s for the child host
and 120 s for the launchd host.

Child host, one `yes` per core:

| Command | Before 1 | After 1 | Before 2 | After 2 | Before 3 | After 3 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Send | 2.9 / 71.1 / 151.5 (79) | 3.2 / 15.1 / 36.7 (81) | 4.7 / 29.2 / 689 (76) | 13.6 / 50.8 / 165.7 (76) | 1.3 / 102.3 / 137.9 (77) | 2.1 / 7.7 / 13.1 (79) |
| ResolveAttention | 3.2 / 90.9 / 197.3 (41) | 3.5 / 7.5 / 28.9 (42) | 4.5 / 138.1 / 281.5 (40) | 17.7 / 54.4 / 124.3 (38) | 1.4 / 139.2 / 147.4 (40) | 1.5 / 10.1 / 27.4 (42) |
| DismissAttention | 2.9 / 56.3 / 153.8 (26) | 2.2 / 6 / 8.7 (28) | 3.9 / 145.4 / 701.2 (25) | 13.7 / 31.4 / 48.5 (22) | 1.1 / 149.6 / 168.4 (27) | 0.8 / 7.8 / 18.1 (28) |
| SettlePausedFile | 3.2 / 114.4 / 209.5 (78) | 3.1 / 7.5 / 21 (80) | 3.8 / 147 / 446.5 (75) | 13.7 / 55.3 / 80.7 (75) | 0.6 / 96.7 / 211.4 (77) | 0.8 / 7.1 / 18.5 (79) |
| DismissPairedNotice | 2.7 / 9.1 / 144.1 (78) | 2.8 / 6.1 / 23.5 (80) | 4 / 54.6 / 466.9 (75) | 12.6 / 55.5 / 93.3 (75) | 0.8 / 9.8 / 53.7 (76) | 0.9 / 5.1 / 23.9 (79) |
| Snapshot | 12.5 / 146.1 / 189.7 (79) | 12.1 / 32.8 / 49.6 (81) | 16 / 370 / 665.7 (76) | 82.9 / 268.5 / 397.5 (76) | 8.4 / 91 / 198.3 (77) | 8.4 / 28.6 / 120.5 (80) |

launchd host, with one `yes` per core in an Interactive job:

| Command | Before 1 | After 1 | Before 2 | After 2 |
| --- | ---: | ---: | ---: | ---: |
| Send | 103 / 664 / 1251.9 (46) | 1.5 / 6.2 / 17.4 (77) | 83.1 / 924.2 / 1635.6 (42) | 1.5 / 4.1 / 11.6 (77) |
| ResolveAttention | 114.1 / 114.1 / 1259.8 (2) | 1.7 / 9.3 / 17.2 (36) | 103.2 / 275.3 / 890.9 (5) | 1.7 / 4.2 / 48.6 (36) |
| DismissAttention | — | 0.8 / 1 / 4.5 (24) | — | 0.7 / 1.2 / 7.9 (24) |
| SettlePausedFile | 110.8 / 792.1 / 1151.4 (45) | 0.6 / 1.8 / 15.8 (77) | 92 / 460.4 / 1809.6 (42) | 0.5 / 4.4 / 16 (77) |
| DismissPairedNotice | 72.6 / 489.8 / 775.8 (45) | 0.7 / 6.6 / 18.6 (77) | 87.5 / 789.8 / 1142.6 (42) | 0.7 / 2.9 / 13.8 (77) |
| Snapshot | 328.7 / 1236.8 / 1527.2 (46) | 7.4 / 16.1 / 36.3 (78) | 517.4 / 1775.8 / 2941.1 (43) | 7.5 / 9.5 / 24.2 (78) |

The throttled host also slowed its agents: they raised few questions, so the before runs have
almost no questions to answer and none left to dismiss.

## Reading

- The signed app's host, the launchd job, now answers every command within 16 ms at p95, against
  0.1–1.8 s before. The `ProcessType` is the larger part of that.
- The child host answers within 33 ms at p95 in two of three runs. In the second run, the load
  average rose from 41 to 60 as other sessions' builds started. Every command stayed within 56 ms
  at p95 except Snapshot, at 268 ms: building a 147 KB snapshot is CPU work, and that run's host
  shared the CPU equally with 12 burners and the extra builds. The client's log shows 167 ms of
  that p95 inside the host and the rest in scheduling the socket threads and the client.
- Commands already went ahead of agent output, so the queue itself was never the wait. Guards:
  `a_device_command_is_taken_before_a_full_queue_of_agent_output` and the tests it sits beside,
  and `a_coordinators_reply_reads_branch_heads_without_running_git`.
