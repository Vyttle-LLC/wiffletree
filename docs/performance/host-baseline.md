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

Reproduce with:

```sh
cargo build --locked --release -p workspace-host --example benchmark
/usr/bin/time -l target/release/examples/benchmark
```

The native GPUI release build succeeds and the development process launches. An idle debug process with five empty sessions was observed at ~79 MiB RSS and a point-in-time 0.0% CPU reading; that observation is not a sustained idle, streaming or renderer-memory benchmark. The Mac was locked, which prevented manual visual QA.

Remaining gates: cold launch to a painted usable window; selection/tab latency; received-event-to-visible latency and UI batch processing; eight simultaneous streams and slow consumers; 50,000-line selectable diffs; graphical/paginated 100,000-commit history; file import; usage ledger and charts; provider overhead and actual authentication; accessible keyboard navigation and input methods; sustained idle CPU; GPU allocations; and the eight-hour memory plateau/soak test. GPUI remains the starting implementation pending this evidence. No equivalent Tauri prototype is warranted by the resolved Metal toolchain setup issue alone.
