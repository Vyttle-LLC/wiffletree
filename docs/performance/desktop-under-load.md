# The desktop window under heavy load

Measured November 27, 2026 on an Apple Silicon Mac with twelve logical CPUs and 36 GiB of
memory, macOS 27, release builds. Other agents' builds and test suites ran throughout, with load
averages of 27 to 65. These are development-machine samples, not a performance guarantee.

## Why it was measured

The window was reported to lock up when the Mac is busy. The question was whether the desktop's
main thread stalls under load, and on which of the paths a person uses: typing in the composer,
Send, settling Inbox items, switching sessions, and the refresh after each host event.

## How stalls are measured

`WIFFLETREE_RENDER_COUNTS` turns on the development stall watch (`render_counts.rs`). It times
each wake of the main run loop, from when it stops waiting to when it waits again, using a Core
Foundation observer. Each wake longer than a 60 Hz frame (16.7 ms) prints a line with:

- its wall time and the main thread's CPU time during it. A gap between the two is time spent
  waiting to be scheduled or blocked;
- the views that rendered and the timed stages that ran during it: snapshot decode,
  `WorkspaceIndex::new`, `share()` and request submission.

The earlier watch timed how late a 5 ms timer fired. macOS coalesces timers, so it reported
17–22 ms "stalls" on an idle main thread. Its numbers, including those in `refresh-cost.md`,
are not comparable with these.

## The load scenario

`crates/workspace-desktop/examples/desktop_under_load.py` runs it:

```sh
cargo build --locked --release -p workspace-desktop -p workspace-host --bins \
  --example many_agents --example inbox_questions
python3 crates/workspace-desktop/examples/desktop_under_load.py /tmp/desktop-load
```

The script:

1. Seeds the many-agents fixture (`docs/performance/refresh-cost.md`): 40 sessions in five live
   projects, kept working by `busy_provider.py`. `inbox_questions` gives the five coordinators
   six questions each for the Inbox.
2. Busies every core with one `yes` per core. They run as a launchd job at niceness 0, because
   children of an agent's shell inherit its niceness of 10.
3. Wraps the release binaries in a bundle and opens it with `open`, so Launch Services starts the
   app as a click in the Dock would.
4. Waits 20 s, then drives the window through its automation socket:
   - streaming: 10 s idle while the agents stream;
   - typing: 114 keys, one every 40 ms;
   - Send: three messages, typed and sent with Enter;
   - Inbox: five Dismiss clicks on the conversation's decision card, then the Inbox panel
     opened and closed three times;
   - switching sessions: twelve clicks across three projects in the sidebar.
5. Prints each path's stalls over 16 ms with their causes.
6. Stops the app and reads the store back. The first project's coordinator must have the three
   messages exactly as typed, and five of its six questions must be dismissed.
7. Exits with status 1 when any stall is longer than `--budget-ms`, 50 by default, when the
   stall watch printed nothing, or when the read-back finds a click that did not land.

Leave the window uncovered while it runs; macOS throttles a hidden one. `--from-shell` starts
the app as the script's child instead of through Launch Services, and `--binaries` points it at
another build.

## Results

No product code changed (see "Started from an agent's shell"), so these numbers are both the
before and the after.

Stalls over 16 ms on each path, with all twelve cores busy at niceness 0:

| Path | Stalls over 16 ms | Over 50 ms | Longest |
| --- | ---: | ---: | ---: |
| Streaming, 11 s | 0 | 0 | — |
| Typing, 22 s | 0 | 0 | — |
| Send, 19 s | 1 | 0 | 16.9 ms |
| Inbox, 14 s | 2 | 0 | 20.0 ms |
| Switching sessions, 9 s | 0 | 0 | — |

A second run gave no stall over 16 ms on any path. A third, with the store read-back, ran at a load
average of 170 and stalled longest on the Inbox path, at 22.5 ms; its read-back passed. Across both runs, the
longest stall in each second was 7–8 ms at the median and 15–18 ms at p95. The main thread used
about 150 ms of CPU a second.

Every stall over 16 ms was a frame that redraws the whole window, with the main thread on the CPU
for nearly all of it, as in `18.7 ms (cpu 18.3 ms): Conversation, ConversationHeader, Inspector,
Sidebar, Transcript, Workspace`. The stages around the host were never the cause:

| Stage | Longest |
| --- | ---: |
| Submitting a request to the host | 0.4 ms |
| Decoding the snapshot | 0.7 ms |
| `share()` into every view | 0.9 ms |

A `sample` profile put about 75% of the main thread's busy time in GPUI's layout, prepaint and
paint, and under 10% in the app's own `render` functions.

Send and Inbox actions mark their item as sending before the request goes out, in the same
update as the click. The frame drawn next shows it. With no stall near 50 ms, that frame comes
at once.

Typing drops no keys. A key from the keyboard waits in AppKit's event queue until the main thread
takes it, so a stall delays keys but never loses them. The scenario's keys go in through GPUI's
keystroke dispatch on the main thread, which cannot drop them either. What typing can show is
lag, and the typing path had no stall over 16 ms. The read-back confirms the three messages
reached the host with every key, in order.

## Started from an agent's shell

Started with `--from-shell` from an agent's shell, the app inherits niceness 10. Its main thread
then runs at the Utility class and waits behind every normal-priority process:

| Path | Stalls over 16 ms | Over 50 ms | Longest |
| --- | ---: | ---: | ---: |
| Streaming | 2 | 0 | 22.1 ms |
| Typing | 78 | 31 | 1140 ms |
| Send | 125 | 59 | 579 ms |
| Inbox | 121 | 78 | 506 ms |
| Switching sessions | 72 | 51 | 679 ms |

In these stalls the main thread used a few milliseconds of CPU over hundreds of wall-clock
milliseconds, as in `1140.3 ms (cpu 4.6 ms)`. It was waiting for the scheduler, not working.
Setting the main thread to the user-interactive class at startup did not help, with longest
stalls of 214–1297 ms per path, so that change was not kept: an unprivileged process cannot lower
its own niceness. This only affects an app started by a lowered process, such as an agent
driving it for tests. A person's launch through the Dock or Finder runs at niceness 0.

## Reading

- On a busy Mac, the desktop's own main thread does not lock up. No path stalled for 50 ms, and
  the work around the host (submitting requests, decoding snapshots, sharing them) stays under
  1 ms.
- Buttons that sit on "Sending…" are waiting for the host's reply, which the host ticket on
  command latency covers.
- What remains above 16 ms is the occasional whole-window frame at 17–25 ms. The cost is GPUI's
  layout and paint of the uncached conversation, so cutting it means caching more of the tree.
  That was not done here, because nothing measured missed the 50 ms budget.

## The regression guards

- In CI, with the desktop tests: two tests use a host that holds every request, as a busy host
  does. A click on an Inbox card's Dismiss button, and Enter in the composer, each show their
  item as sending in the frame of the input. Each then settles once the host answers.
  - `a_dismissed_card_shows_it_is_sending_at_once_while_a_busy_host_holds_the_reply`, in
    `inbox.rs`.
  - `a_sent_message_shows_it_is_sending_at_once_while_a_busy_host_holds_the_reply`, in
    `conversation.rs`.
- In the benchmark harness, not CI: `desktop_under_load.py` fails when a stall passes the 50 ms
  budget. Stall timing needs a window on screen on a quiet machine, which CI runners do not
  give reliably.
