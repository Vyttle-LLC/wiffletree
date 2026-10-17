# Phase 4b host revisions and test harness

Measured October 17, 2026 on an Apple M4, 24 GiB memory, ten logical CPUs,
macOS 27.0.1, Rust 1.97.0, using the normal release profile and build cache.
These are development-machine samples, not a performance guarantee. Neither run
overlapped another build from this session; other sessions share the machine.

Both runs used the existing benchmark's 750 sessions, 140 tickets, 100,000
messages and 100,000 activity rows. The before runtime is `91823f59`; after is
`b71ddaed`'s maintained revisions, with the keeper polling changes (which have no
watched repositories in this fixture). Each snapshot measure has 200 samples.
These measurements precede the rebase onto Phase 3 plus its fixture-provider CI
fix, `955ec546`; that rebase preserves the measured archive and message revision
implementation.

| Operation | Before p50 | Before p95 | After p50 | After p95 |
| --- | ---: | ---: | ---: | ---: |
| Snapshot host build | 8.175 ms | 9.034 ms | 2.712 ms | 2.934 ms |
| Snapshot with JSON round trip | 8.566 ms | 9.655 ms | 2.910 ms | 3.163 ms |
| Snapshot JSON size | 192,495 B | | 196,599 B | |

Host build p50 fell by 66.8%. The additive message revision map accounts for the
snapshot's roughly 4 KiB increase. Full results: [host-revisions.json](host-revisions.json).

Reproduce with:

```sh
cargo build --locked --release -p workspace-host --example benchmark
target/release/examples/benchmark
```

## Archive invalidation

`archive_stamp` now reads one durable revision row. Connection-local SQLite triggers on the host's single writer advance it
when archived session, ticket or runtime rows change, including indirect archive
membership changes when a ticket settles, an agent is restored or a runtime's
ticket changes. They ignore byte-identical writes and roll back with their
transaction. Turns have a durable membership record so entering or leaving a turn
also invalidates archived rows, including a restart ending an archived turn whose
stored status was already done or blocked. Live-only changes leave it alone.

The production archive hash is removed. Its test-only oracle compares whether the
old hash and revision move at each step of a scripted lifecycle. A second test
covers two agents' archive flags, open/closed/accepted tickets and runtime
membership changes, including a restored agent keeping a settled ticket live.
As before, appending live rows to the archive's ordering lists alone does not
invalidate its contents; the client places those new rows after cached rows.
Revision values are opaque: clients compare equality, never increments or hashes.

## Message revision contract for Phase 4a

`Snapshot.message_revisions` is an additive JSON object mapping session IDs to
unsigned 64-bit revisions. It contains every session in that snapshot, including
open tickets' archived agents and archived sessions still in a turn. Archive-only
sessions have no entry. An absent field decodes as an empty map.

- Each revision is durable and belongs to the recipient's complete stored
  transcript, including older pages. Message inserts, actual edits (body,
  receipt, attachments or other stored message fields) and deletions advance it.
  It covers final-reply insertion and replacement, compaction notices, queued and
  held receipt changes, and recovery writes through the same host connection's
  database triggers. Internal `quiet` metadata is not part of message pages.
- No-op writes, duplicate sends and unrelated sessions' changes leave it alone.
  Failed transactions also roll back revisions. A new or migrated session starts
  at zero until its first message mutation after this migration; zero is a valid
  revision for its current transcript, not an indication that it is empty.
- A client fetches on first selection, after losing its transcript cache, when the
  revision differs from the last successfully fetched version, or when the entry
  is missing. Compare equality only. Associate a completed fetch with the revision
  that prompted it; do not credit a newer snapshot before its fetch succeeds.
- This signal describes messages. Panels with other data retain their own
  invalidation rules. General change notifications and message commands remain
  unchanged, so clients that ignore the field continue to refetch correctly.

Tests cover session isolation, rejected and duplicate sends, receipts, edits,
final-reply replacement, no-op writes, deletion, rollback, restart and decoding a
snapshot from an older host.

## Keeper polling

`checkout::KeeperPoll` prepares ticket, repository and watched-base inputs on the
actor, runs HEAD, ancestry, origin and publication-eligibility reads in a job, and
revalidates those inputs before making keeper work due. A changed input starts a
fresh inspection. Concurrent requests for the same repository coalesce and use
the latest poll time. A reproduction's completion uses a fresh inspection before
starting a fix. A failed inspection starts no fix and releases polling or cleans
up the reproduction through its existing discard path.

Tests hold a Git ancestry query while an actor snapshot completes, check
coalescing, reject stale ticket and base inputs, and exercise panic completion.
The existing keeper lifecycle suite checks rebase/fetch/failed-check behavior.
The intentional worktree-removal transaction remains synchronous.

## Test harness

Counts are free helper definitions in host integration tests, including common;
methods on a test's domain fixture are not counted.

| Helper | Before | After |
| --- | ---: | ---: |
| `request` | 9 | 1 |
| `snapshot` | 7 | 1 |
| `until` | 10 | 1 |
| `git` | 13 | 1 |
| Unit-test `git` | 6 | 1 |
| Integration binaries importing common | 5 | 18 |

Common also has named snapshot-predicate and change-signal wait variants, a raw
Git-output helper for tests asserting failure, and a small `ServiceFixture`
builder. Tests keep their store setup and domain assertions. Custom providers
keep their scripts and environment locks; the builder shares provider paths and
service startup. Desktop test helpers are untouched.

The Phase 3 follow-up replaces Admission's session-ID set and general dirty flag
with one dirty flag. Admission only needs to know whether another pass is due;
the existing admission tests cover ignored reads/streaming, finishes, timer
matching and consuming the signal. Phase 3's repeated admission pass, sorting
launch and compaction usage fixes are retained by the rebase.

## Validation

All four requested checks passed on the final source:

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked -p workspace-host && python3 scripts/test_communications.py
```

Revalidated after rebasing onto Phase 3 plus its fixture-provider CI fix
(`955ec546`) and simplifying Admission to one dirty flag. The workspace run
passed 1,255 tests with four ignored tests (including the two service tests run
by their fixture-provider wrappers). The communications fixture completed its
cross-provider handoff in 0.885 seconds and
passed work steps, usage/quota, questions, timer, permission, exact resume,
cancellation and restart checks. Existing legacy-store fixtures also pass without
desktop changes: optional message columns can be migrated while a host is closed,
and the host reinstalls its connection-local triggers after schema migration.

The entire workspace test run also disabled real-provider discovery: PATH held
only Python 3.13, `/usr/bin` and `/bin`, Cargo and Rust tools used explicit paths,
and both `WORKSPACE_CLAUDE_BIN` and `WORKSPACE_CODEX_BIN` pointed to a verified
nonexistent file. Explicit provider overrides are checked before PATH, HOME,
Homebrew or `/usr/local/bin` fallbacks, so no installed provider could be selected.
Fixtures replace those overrides with their own providers. This passed all 639
host unit tests and all 304 tests in the 27 host integration binaries, including
`slow_worktree_preparation_preserves_input_and_defers_its_archive_until_git_finishes`.
The two service wrappers each also execute their ignored child test and verify
its success.
