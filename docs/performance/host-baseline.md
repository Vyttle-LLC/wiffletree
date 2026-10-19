# Initial host performance baseline

Recorded October 2, 2026 in America/New_York. These are application-host measurements, not a claim of zero lag or a passed desktop performance gate.

Apple M3 Pro (Mac15,6), 36 GiB memory, 12 logical cores; macOS 27.0 (26A428); Rust 1.97.0. Release build with thin LTO. SQLite WAL and `synchronous=FULL`; local temporary filesystem; warm indexed read queries. A native release build overlapped part of the run, so these are development-machine observations rather than controlled laboratory results.

The fixture contains five projects, ten repository metadata attachments, 50 sessions, 100,000 messages in one transcript, 100,000 activity records and 100,000 source-memory entries. Repository attachments are synthetic metadata for this benchmark; it does not measure Git history. Fixtures are seeded in a bulk transaction, then each measured enqueue uses its own durable host transaction. Messages are 256 bytes in retained history and 4 KiB for measured enqueue.

| Operation | Samples | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Transcript page, 100 rows | 1,000 | 0.060 ms | 0.067 ms | 0.197 ms |
| Workspace snapshot, 50 sessions | 1,000 | 0.043 ms | 0.046 ms | 0.100 ms |
| Activity page, 100 rows | 1,000 | 0.044 ms | 0.091 ms | 0.325 ms |
| Source-memory page, 100 rows | 1,000 | 0.091 ms | 0.094 ms | 0.135 ms |
| Durable 4 KiB enqueue | 200 | 0.109 ms | 0.154 ms | 1.224 ms |
| Host reopen, recovery and snapshot | 100 | 12.585 ms | 14.873 ms | 16.228 ms |

The benchmark process peaked at 11,255,808 bytes (~10.7 MiB) resident memory according to `/usr/bin/time -l`. This includes fixture construction and host queries; it excludes the desktop, GPU and providers. It is not the PRD's combined wrapper memory measurement. “FULL” describes SQLite's configured synchronization, not a separate test of power-loss durability.

Raw values and environment: [host-baseline.json](host-baseline.json).

## Work steps — October 7, 2026

The fixture adds 101 finished runs with 4,000 stored steps: one run at the 2,000-step limit and 100 runs of 20 steps. These rows come from a later run on a different Mac (Apple M4, Mac16,10, 24 GiB memory, 10 logical cores; macOS 27.0.1 (26A434); Rust 1.97.0; release build), so compare them with each other rather than with the table above. In the same run, the existing measures stayed within about 0.02 ms of the table above, and host reopen with step recovery measured p50 12.347 ms and p95 13.137 ms.

| Operation | Samples | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Step page, whole 2,000-step run | 1,000 | 2.194 ms | 2.317 ms | 3.915 ms |
| Step page after cursor, nothing new | 1,000 | 0.124 ms | 0.130 ms | 0.155 ms |
| Reply summaries, 100 runs | 1,000 | 1.284 ms | 1.432 ms | 2.292 ms |

Peak resident memory was 16,596,992 bytes (~15.8 MiB). Not measured: the service actor's in-memory merge per streamed update, eight simultaneous streams, and the desktop's per-batch main-thread time against the 8 ms p95 target. In a manual run, a replayed recorded Claude turn of 129 stream events produced 73 paced step fetches and no step-triggered snapshot reloads; the snapshot reloads that did occur came from per-request usage events.

## Bounded snapshot — October 17, 2026

The snapshot now holds only live work, the archive is read apart from it and only while a view needs it, and the desktop joins the two in `WorkspaceIndex`. The fixture gains a finished store's shape: 140 tickets, ten of them open, each with an implementer and four verifiers, so 750 sessions with the coordinators and workers, 690 of them archived, and a runtime for every ticket agent. Ticket rows are sized like the human's (a 4,400-character brief, two cycles, up to 18 ledger entries). Before is Phase 1's head (`87b85177`) with this fixture, after is this change; both ran on the Apple M4 above (macOS 27.0.1, Rust 1.97.0, release build) on the same day.

| Operation | Before p50 | Before p95 | After p50 | After p95 |
| --- | ---: | ---: | ---: | ---: |
| Snapshot JSON size | 3,641,867 B | | 192,495 B | |
| Workspace snapshot, host build | 37.127 ms | 38.709 ms | 7.616 ms | 8.036 ms |
| Snapshot with JSON round trip | 40.783 ms | 42.195 ms | 7.884 ms | 8.550 ms |
| Archive with JSON round trip (3,484,472 B), read only while a view needs it | | | 18.060 ms | 19.828 ms |
| Index of snapshot and archive | | | 0.209 ms | 0.222 ms |
| Sidebar tree rows | 25.986 ms | 26.076 ms | 0.008 ms | 0.011 ms |
| Sidebar tree rows, Show archived | 50.822 ms | 51.187 ms | 0.093 ms | 0.099 ms |
| Host restart and snapshot | 104.830 ms | 110.962 ms | 78.994 ms | 86.740 ms |

The after column was measured again later that day with no other builds running, at `2261b4aa`, which adds Phase 3a's desktop work and Phase 3b's host commits. It includes review's change that keeps open tickets' archived agents, such as an earlier cycle's verifiers, in the snapshot, so that the default sidebar needs no archive. That change added 40 sessions and their runtimes to the fixture's snapshot. Before it, the snapshot was 152,783 bytes and built in 5.498 ms p50.

On a read-only copy of the human's store, measured before that review change (137 tickets, 718 sessions, 714 archived), the snapshot went from 2,148,697 to 100,681 bytes and its round trip from 14.357 to 4.325 ms p50. Tree rows with Show archived went from 54.480 to 0.098 ms p50, and without it from 0.816 to 0.001 ms. A fresh copy measured with the after column holds 142 tickets and 728 sessions, 723 of them archived. Its snapshot is 181,175 bytes and round-trips in 7.266 ms p50 (7.587 ms p95). The archive is 2,048,423 bytes and round-trips in 7.823 ms, the index builds in 0.207 ms, and tree rows take 0.101 ms with Show archived and 0.002 ms without.

Of the old host build, 19 ms was `questions_waiting` scanning every message and 15 ms `undelivered` doing the same; both now go through existing indexes (message ids, and each session's outbox). Accepted and closed tickets and archived sessions are found through two expression indexes. Most of what remains is hashing the archive's stored rows for `archive_stamp`, about 4.5 ms, and finding open tickets' agents. Tree rows and the snapshot sizes and round trips on the desktop side come from the ignored `tree_rows_benchmark` test, pointed at a kept fixture or store copy; the other rows come from the benchmark. The desktop decodes the snapshot and builds the index on its main thread; that is the round trip's decode plus the index, under 1 ms here.

Phase 4b replaces the remaining archive hash with a maintained revision. Its
[same-fixture before/after measurements](host-revisions.md) reduce snapshot host
build p50 from 8.175 ms to 2.712 ms, including per-session message revisions.

## Over host.sock — October 19, 2026

The desktop now reaches its host only through `host.sock` (`openspec/changes/archive/2026-10-19-standalone-host`, task 4.2). The benchmark gained two measures: a child `workspace-host serve` started on the same fixture, as the app starts one, and asked through `workspace_core::HostClient`. Each sample covers the request written to the socket, the host's actor and JSON, the reply line and the decode the desktop does. The run was on the Apple M4 above (macOS 27.0.1, Rust 1.97.0, release build) at `f965b945`. Other agents' release builds were running alongside, so the p95 and maximum columns are noisier than the October 17 run.

| Operation | Samples | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Transcript page, 100 rows, in process (no JSON) | 1,000 | 0.061 ms | 0.181 ms | 0.373 ms |
| Transcript page, 100 rows, over `host.sock`, decoded | 1,000 | 0.965 ms | 1.497 ms | 88.869 ms |
| Snapshot with JSON round trip, in process | 200 | 6.330 ms | 12.433 ms | 20.493 ms |
| Snapshot over `host.sock`, decoded | 200 | 6.645 ms | 20.822 ms | 52.267 ms |

The 197,772-byte snapshot costs about 0.3 ms more at p50 over the socket than its in-process JSON round trip. A transcript page adds about 0.9 ms: the host's actor, the JSON of 100 messages each way and the socket, which the in-process figure skips and this run does not tell apart. At p50 both stay well under a 60 Hz frame (16.7 ms). At p95 the transcript page does too, but the snapshot over the socket took 20.8 ms, against 12.4 ms in process, on this loaded machine. The maxima are single outliers on a loaded machine; a quiet rerun should replace them before they are compared with a target.

Reproduce with:

```sh
cargo build --locked --release -p workspace-host --bin workspace-host --example benchmark
/usr/bin/time -l target/release/examples/benchmark [folder to keep the fixture in]
WIFFLETREE_BENCH_DIR=<kept folder or a store copy> cargo test --locked --release -p workspace-desktop tree_rows_benchmark -- --ignored --nocapture
```

The native GPUI release build succeeds and the development process launches. An idle debug process with five empty sessions was observed at ~79 MiB RSS and a point-in-time 0.0% CPU reading; that observation is not a sustained idle, streaming or renderer-memory benchmark. The Mac was locked, which prevented manual visual QA.

Remaining gates: cold launch to a painted usable window; selection/tab latency; received-event-to-visible latency and UI batch processing; eight simultaneous streams and slow consumers; 50,000-line selectable diffs; graphical/paginated 100,000-commit history; file import; usage ledger and charts; provider overhead and actual authentication; accessible keyboard navigation and input methods; sustained idle CPU; GPU allocations; and the eight-hour memory plateau/soak test. GPUI remains the starting implementation pending this evidence. No equivalent Tauri prototype is warranted by the resolved Metal toolchain setup issue alone.
