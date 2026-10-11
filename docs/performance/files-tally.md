# Files panel tally cost

Recorded October 11, 2026 in America/New_York. This is the host-thread cost of the store part of a Files request, not a measurement of Git reads, the reply, or the desktop.

Apple M3 Pro, 12 logical cores, 36 GiB memory; macOS 27.0.1; Rust 1.99.0; release build. Local temporary filesystem, SQLite WAL, warm cache.

## Why it was measured

While the Files panel is open, the desktop asks for files at most twice a second. For each agent in the reply, `Host::file_activity` tallied its stored read and edit steps on the host thread, the thread that also serves messages and streaming. The PRD's budgets for tab response and for a received event to a visible update are p95 under 100 ms. A reviewer had seen about 80 ms per session at 20,000 steps in a scratch database.

## Method

`files::tests::tally_cost` (ignored by default) times `Host::ticket_files`, the whole store part of a request, 40 times per case on a fixture stored through the real `steps` table, with each step about 700 bytes of JSON including a 512-byte detail. In the "mixed" fixture two of every five steps read or edit one file and the rest are commands. In the "worst" fixture every step reads or edits two files. Paths are spread over 150 files in 7 directories. Reproduce with:

```sh
cargo test --release --locked -p workspace-host --lib -- --ignored --nocapture tally_cost
```

## Results

p50 / p95 in milliseconds, per request. The two columns are the same fixtures; "SQL JSON" is the original query (`json_extract` and `json_each` in SQLite), "Rust decode" the replacement (select each step's JSON and read only kind, state and paths).

| Case | SQL JSON | Rust decode |
| --- | ---: | ---: |
| One agent, one run of 2,000 mixed steps | 2.74 / 2.87 | 0.81 / 0.91 |
| One agent, 20,000 mixed steps (ten runs) | 36.57 / 39.37 | 10.11 / 10.51 |
| One agent, 20,000 worst-case steps | 113.79 / 117.26 | 14.04 / 14.28 |
| Coordinator rollup: five agents, four of them with 20,000 worst-case steps | 378.15 / 398.26 | 55.14 / 68.91 |

## Reading

- The original query missed the 100 ms budget for one worst-case agent and by a wide margin for a rollup, and it did so up to twice a second.
- Decoding only the needed fields in Rust meets it in every case measured. An ordinary agent costs about 1 ms; the rollup case, which is already far beyond the 50-session and 8-stream fixture (20,000 steps is ten full runs, and a run stores at most 2,000), costs about 55–69 ms.
- A coordinator with more than about eight agents each holding 20,000 steps would exceed 100 ms again. Not measured, and not expected in practice; the next step then would be to move the tally to the reply thread, which needs its own read-only connection. No schema change was needed.
- Not measured: Git reads (already off the host thread), serializing the reply, or contention with eight concurrent streams.
