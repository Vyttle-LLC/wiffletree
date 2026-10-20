# Desktop refresh cost with many agents

Measured October 20, 2026 on an Apple M4, 24 GiB memory, ten logical CPUs, macOS 27.0.1,
Rust 1.97.0, release builds. These are development-machine samples, not a performance guarantee.
Other sessions' test suites ran throughout: the load average stayed between 8 and 36, so stall and
CPU figures vary run to run. Render and notify rates repeat closely.

## Why it was measured

The desktop felt slow with many agents running. The suspect was the snapshot path: each state
event refetches the whole `Snapshot`, decodes it, rebuilds `WorkspaceIndex` and shares it with
every view.

## Fixture and method

`crates/workspace-host/examples/many_agents.rs seed <new folder>` builds a scratch store:

- five live projects, each with a coordinator, a small Git repository without an origin, and
  seven tickets;
- one Claude implementer on each ticket, with some history;
- 40 sessions in all.

`busy_provider.py`, set as `WORKSPACE_CLAUDE_BIN`, keeps the agents working without inference:

- each turn streams 20 narrated commands half a second apart;
- it records a checklist every five steps and reports progress halfway;
- workers then report blocked, and coordinators message their idle workers, so turns never stop.

`many_agents probe <folder> [seconds]` joins the running host as a script client. It counts
events and times `Snapshot` over `host.sock`, five times a second.

With `WIFFLETREE_RENDER_COUNTS` set, the desktop prints once a second:

- each view's renders and notifies;
- the decode, `WorkspaceIndex::new` and `share()` timings;
- main-thread CPU time;
- the longest main-thread stall, measured as how late a 5 ms timer fires.

Each run seeds a fresh store, opens the app on it with the coordinator of the first project
selected, waits 20 s and measures 60 s:

```sh
cargo build --locked --release -p workspace-desktop -p workspace-host --example many_agents
target/release/examples/many_agents seed /tmp/many
WORKSPACE_CLAUDE_BIN=$PWD/crates/workspace-host/examples/busy_provider.py WIFFLETREE_RENDER_COUNTS=1 \
  target/release/workspace-desktop --data-dir /tmp/many/home 2> /tmp/many/desktop.log &
target/release/examples/many_agents probe /tmp/many 60
```

Only runs where the window was on screen count. A hidden window renders nothing, and macOS
then throttles its timers: those runs showed 100 ms "stalls" at 11 ms/s of main-thread CPU.

## Load

About 24 of the 40 agents were working at a time. The host sent about 15 state, 155 step,
16 progress and 2.5 usage events a second. The snapshot was 139 KB, answered over the socket in
p50 2–4 ms and p95 6–26 ms.

## Baseline: the snapshot path is cheap

| Stage, per refresh | Average |
| --- | ---: |
| Decode `Snapshot` | 0.07 ms |
| `WorkspaceIndex::new` | 0.03 ms |
| `share()` into nine views | 0.03 ms |

The app refreshed about 10 times a second and shared about 25 times a second, since progress
pages share too. That comes to about 2 ms of main-thread CPU per second.

The renders those refreshes cause cost more. An A/B with local toggles, not kept, came from one
run each:

| Run | Main-thread CPU | p95 stall |
| --- | ---: | ---: |
| Everything on | 140 ms/s | 12.0 ms |
| State refreshes off | 117 ms/s | 12.6 ms |
| Step wakes off | 64 ms/s | 9.1 ms |

Step wakes were the larger cost. Every step event, for any session, made the selected
transcript fetch its steps, up to 30 times a second. Each reply notified the transcript even when
the page held nothing new, which redraws the Conversation and Workspace around it. Snapshot
pacing, worth at most about 23 ms/s, was not done.

## Change

- The bridge now records whose steps each event names, or everyone's after a resync.
- The transcript fetches steps only when the selected session's changed. The Files panel fetches
  only when its session's, or for a coordinator its agents', changed.
- `RunActivity::merge` reports whether anything shown changed, and the transcript notifies only
  then.

## Before and after

The before and after binaries ran alternately on the same fixture:

| Run | Workspace renders/s | Transcript notifies/s | Main-thread CPU | p95 stall | Max stall |
| --- | ---: | ---: | ---: | ---: | ---: |
| Before 1 | 34.9 | 51.7 | 203 ms/s | 15.3 ms | 16.3 ms |
| After 1 | 26.2 | 32.4 | 116 ms/s | 11.0 ms | 11.2 ms |
| Before 2 | 34.9 | 51.4 | 140 ms/s | 12.0 ms | 28.2 ms |
| After 2 | 22.2 | 30.1 | 140 ms/s | 15.1 ms | 15.9 ms |

Two more after runs gave 24.5 and 24.8 Workspace renders a second at 119–120 ms/s of
main-thread CPU, with p95 stalls of 11 ms. No run missed more than one 60 Hz frame a minute.

## Reading

- Whole-window renders fell by about 30% and transcript notifies by about 40%, consistently.
  Main-thread CPU and stalls moved within this machine's noise, so no stall improvement is
  claimed.
- The PRD's p95 target of 8 ms of main-thread work per UI update batch while streaming is not
  directly measured. The longest stall per second, a frame's render included, stayed at 11–15 ms
  at p95, and frames were not missed.
- What remains is mostly `share()`. Each snapshot and progress page hands every view a new
  `Shared`, and the Sidebar (about 36 notifies a second) and transcript notify on it, whether or
  not what they draw changed.
- Nothing measured here stalls the UI for long. A multi-second freeze is better explained by
  blocking host calls on the UI thread, such as file-link resolution.
