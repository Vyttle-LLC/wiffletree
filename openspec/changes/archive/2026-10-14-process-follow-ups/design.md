## Context

Five follow-ups from the reviews of #62 (`2026-10-13-stop-agent-processes`) and #59 (`2026-10-13-spawned-processes-own-descriptors`). Each is small. This records the choices and the facts they rest on, measured on macOS 27.0.1.

## 1. Freeze, look, stop

On a cancel, `run_turn` returns `Cancelled` and the dropped `Process` stops the provider's group. Only then does `run` call `watch.finish`. A tool the provider forked into a session of its own within one scan interval before that, about 4 ms, is orphaned by the stop before any scan has listed it. It is then neither a descendant nor in a group the tree holds.

`Process::stop` now calls `processes::stop_provider_group(group, processes::look)`:

1. `killpg(group, SIGSTOP)`, then wait until every live member of the group reads `SSTOP`, for up to 100 ms. A process stops on its way back to user mode, so one that has stopped is not part way through a fork.
2. `look()`: one scan of the shared watcher. The frozen provider is alive and is still the parent of anything it forked, so the scan adopts the tool into the run's tree. Its sighting goes out with the run's last sightings, before the turn's end, and the host stops it when the cancelled turn ends, as for any group recorded since the stop.
3. `terminate`: SIGTERM, then SIGCONT, then the grace period, then SIGKILL as before.

This applies to every stop of a provider's own group, not only a cancel. At a normal end the leader has exited and the freeze covers the helpers left in its group, at the cost of one scan.

**SIGTERM to a stopped process.** Measured with `sh` as the stand-in: a stopped process with no SIGTERM handler is ended by SIGTERM at once, without being continued. One with a handler, as Claude Code's Node process has, holds the signal until it is continued. So `terminate` sends SIGCONT after SIGTERM: the signal is pending when the process resumes, and it acts on it before running anything else. `stop_group` uses the same `terminate`, so startup, archive, stop and quit also stop a frozen group with SIGTERM instead of waiting a second for SIGKILL.

**Never left frozen.** The freeze is a guard whose `Drop` sends SIGCONT, so a look that panics continues the group. If the host itself dies in the fraction of a millisecond between the freeze and SIGTERM, nothing can continue the group until a host runs again. The group is still recorded as the run's provider group, and the next startup stops it with SIGTERM and SIGCONT. A supervisor process would close that window; it is not worth one.

**Best effort under load.** The wait for the members to stop is bounded at 100 ms, so a host on a stalled machine does not hold up a stop. If a member has not stopped by then, the look runs anyway, and a fork that member makes after the look is missed as before. A group that still runs but whose members cannot be read, as during an exec, does not count as stopped.

**What it does not close.** The freeze finds what a member of the provider's group forked. A tool shell that is itself forked, starts a child and exits, all between two scans, is still the known gap that a shell the watcher never lists leaves.

## 2. A replaced verifier is stopped after the review starts

`verify_ticket` archives a verifier it replaces with `change_archived`, inside the one `atomically` savepoint that starts the review. `change_archived` stops what the verifier started, which cannot be undone. When a later step failed, the rollback erased the stop's record and its notice, and the verifier stayed active with its processes gone and no word of it.

Two fixes were possible: stop first and settle afterwards, as `finish_ticket` does with `archive_failed`, or stop only once the change has committed. The second is chosen. A review that fails to start then changes nothing at all, and a refused or failed review should leave the verifier as it was. `change_archived` keeps its behaviour. `archive_in_change` archives without stopping and returns the archived sessions, and `stop_archived` stops what they started once the caller's change has committed. A verifier in its turn is skipped as before, and its turn's end stops its processes.

**The held verifier cannot happen today.** The brief's example is a later verifier held because its provider is disabled. `verifier_choices` checks every choice with `permits_verifier` before the savepoint, which refuses a disabled provider, and only a verifier pinned to the chosen profile is reused. So `hold_reason` inside the loop cannot fail through a disabled provider. The test therefore makes the round's first message fail to store, using an SQLite trigger in the test's own store. That failure comes after the replacement, which is what matters.

## 3. The desktop app's own processes

The host runs on a thread of the desktop process (`Service::start`), so a process the desktop starts can inherit a provider's half-made pipe. The archived design said otherwise and is corrected.

| Site | Hook | Why |
| --- | --- | --- |
| `update.rs`: `curl` (feed and download), `ditto`, `codesign` (verify and team), the swap shell | Yes, through `command` | They run while the host does. The swap shell lives until the app exits. |
| `login_env.rs`: the login shell | No | `adopt` runs first in `main`, before any other thread exists, so no pipe can be half-made. |
| `leftovers.rs:301`, `tree.rs:1008`: `git` | No | Test helpers. |
| `update.rs` tests: `true`, `sh` | No | Test code. |

`OwnDescriptors` is now public in `workspace_host::runtime`.

## 4. The fallback of the descriptor hook

The hook's list size becomes a const parameter, `close_others_on_exec::<1024>` in production. The test runs `ls /dev/fd` with a list of four while eight inheritable pipe ends are open. The list fills, so the hook marks every descriptor from 3 to the limit. The child holds 0 to 2 and the two `ls` opens.

## 5. The restart test starts its turns together

Both messages are queued before the headless host starts, so both providers spawn together, as before #59. The tools are identified from the record rather than from the order of the shared pid file, which is no longer fixed.

## Shown to fail without each fix

| Test | Without the fix |
| --- | --- |
| `a_tool_forked_just_before_a_cancel_is_found_by_the_stops_look` | Stopping before the look, with no freeze: the look names nothing, and the tool's parent is launchd. |
| `a_group_left_frozen_is_stopped_with_sigterm` | No SIGCONT: the handler never runs and the group is killed after the grace period. |
| `a_stop_whose_look_fails_leaves_the_group_running` | No continue on drop: the group stays stopped. |
| `a_review_that_fails_after_replacing_a_verifier_leaves_what_it_started_running` | `change_archived` in the loop: the process is killed and the record says nothing. |
| `its_commands_leave_the_child_only_its_standard_streams` | No hook: the child holds descriptor 100. |
| `a_child_whose_descriptor_list_may_be_cut_short_marks_every_descriptor` | No fallback loop: the child holds descriptors 3 to 12. |

The restart test cannot fail on demand. It hung about 1 run in 400 before #59; see the tasks for the runs made.

## Fixed in review, round 1

- The freeze counted a group as stopped when none of its members could be read, which happens while a member is inside an exec, so the look could come before anything had stopped. It now counts such a group as stopped only once the group no longer runs. Its test froze `echo ready; exec sleep 30` as soon as `ready` was read, possibly mid-exec, and failed about once in 100 runs; it now waits for the exec first.
- `archiving_stops_orphans_whose_group_leader_exited_before_the_turn_ended` failed on a slow CI runner: its `LEAVE_ORPHANS` tool shell exited a few milliseconds after it started, sometimes before the watcher had listed it, which is the documented gap. Letting the shell live 100 ms was not enough: at a load average near 80 it still failed once in 150 runs, because the watcher thread itself went unscheduled for that long. The shell now exits only when the test has seen it and its two children recorded, and the provider waits for it to exit before the turn ends, so the test still shows a group whose leader exited first. The other modes' shells exec a long-lived process; `BRIEF_SHELL` tests the gap on purpose and is unchanged.

## After release: the brief tool shell

The v0.9.0 Release failed on `a_tool_shell_that_lives_a_few_scan_intervals_is_seen_and_its_child_recorded`: its `BRIEF_SHELL` lived 20 ms, and the watcher thread went unscheduled for all of it, so nothing was recorded. That is the same starvation as above, and no lifetime is long enough to rule it out, so the test no longer depends on time. It is now `a_child_started_by_a_tool_shell_on_its_way_out_is_recorded_in_the_shells_group`: the shell waits until the test has seen it recorded, then starts its child and exits at once. Only the watcher's own scans record anything while a turn runs, so the test still fails if they stop. How short a shell the watcher catches rests on deterministic unit tests instead: `a_shell_that_starts_and_exits_between_two_scans_is_never_seen` documents the gap and bounds `SCAN_INTERVAL` at 5 ms, and `a_child_whose_shell_exited_between_two_scans_is_found_in_its_group` covers the child that only its group links to the provider.

Looking into it found a gap in the watcher itself. A process that cannot be read when first listed, as happens part way through an exec, was still counted as listed and never read again; had it been a tool shell, its group would have been lost. Such a process is now read again at the next scan.

Two gaps remain, both by design: a shell that starts and exits between two scans is never seen, and a group whose leader exited while the watcher did not look for over `CONTINUITY` is not adopted.
