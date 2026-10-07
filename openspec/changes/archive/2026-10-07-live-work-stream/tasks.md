## 1. Step model and storage

- [x] 1.1 Add `Step`, `StepKind`, `StepState`, and `Command::Steps` with its `StepPage` response to `workspace-core`, and verify that `cargo check --workspace` passes
- [x] 1.2 Add `steps.sql` (table and `(session_id, revision)` index), run it at host open beside `usage.sql`, and verify that a fresh and an existing data directory both open in `cargo test -p workspace-host`
- [x] 1.3 Implement the bounded merge (200-char title, 4 KiB detail tail with an `omitted` count, 2,000 steps per run) and verify with unit tests for the truncation and omission counts
- [x] 1.4 Extend startup reconciliation to mark stored Running steps Interrupted and prune steps whose run finished more than 30 days ago, and verify with a recovery test in `steps.rs` (it needs crate-internal writes)

## 2. Provider adapters

- [x] 2.1 Record Claude and Codex stream fixtures (JSON lines covering text, thinking, tool use and result, errors, subagent parent ids, command, file change, MCP, todo list and unknown events) under `crates/workspace-host/tests/fixtures/`, and verify that the files load in a test
- [x] 2.2 Map Claude stream-json to `ProviderEvent::Step` updates and a single `ProviderEvent::Reply`, and verify with fixture tests that each tool use yields one step whose final state is correct and that only `result.result` becomes the reply
- [x] 2.3 Map Codex `item.*` events to step updates and the last `agent_message` to the reply, and verify with fixture tests, including a nonzero `exit_code` that yields Failed
- [x] 2.4 Verify with a fixture test that unknown and malformed-but-parseable events neither create steps nor fail the turn

## 3. Host streaming

- [x] 3.1 Hold the in-memory steps and the per-session revision in `Active`, persisting on start and state change only, and verify with a test that streams 1,000 text deltas and asserts the SQLite write count stays below a small fixed number
- [x] 3.2 Write `output:{run}` once from `Reply` at `Finished`, use it for the parent notification, and verify in `scripts/test_communications.py` that narration is absent from the transcript
- [x] 3.3 Add a separate bounded step signal beside the state signal, so step events never trigger snapshot reloads, and verify with a test that a burst yields one pending signal and no state signal, and that a change after a read signals again
- [x] 3.4 Serve `Command::Steps` (cursor and optional `run_id` paging, memory merged with stored steps), and verify with tests for the cursor, restart and paging behavior
- [x] 3.5 Add one line to `skills/communication.md` telling agents that only their final message reaches the chat, and verify that the skill text includes it

## 4. Desktop: activity card

- [x] 4.1 Handle the step signal with one in-flight `Steps` request plus a dirty refetch paced to 33 ms, and verify by running the app against a fake provider that no snapshot request is made per step
- [x] 4.2 Replace the working row with the fixed-height activity card (header, narration headline, three latest steps, "View all"), and verify in the running app that its height and the transcript's scroll position stay fixed during a live turn
- [x] 4.3 Add the minimize/restore toggle that keeps the current step on the header line, saved in a new client-side `desktop-preferences.json` (a missing or unreadable file falls back to the full card), and verify in the running app that the choice applies to another working agent and survives a relaunch
- [x] 4.4 Run the 1 Hz elapsed-time timer only while the selected session is Working, and verify in the running app that it stops when the turn ends or another session is selected

## 5. Desktop: history sheet and turn summary

- [x] 5.1 Add `history_sheet.rs`, a right `Sheet` for one run that shows narration paragraphs with their steps beneath (rows follow step order, so grouping needs no separate model) and collapsible details, and verify with a unit test that pages merge in step order and in the running app with a replayed Claude turn
- [x] 5.2 Make the open sheet update live, follow the tail unless the reader has scrolled up, and close on Escape or an outside click while the draft is kept, and verify manually during a streaming turn
- [x] 5.3 Serve `Command::RunSummaries`, and verify with a host test for the counts, failed steps and missing runs
- [x] 5.4 Render `▸ Worked {duration} · {n} steps` above each `output:{run}` reply (hidden when the reply is the only step) that opens the sheet for that run, and verify on an earlier turn after restarting the app

## 6. Validation and docs

- [x] 6.1 Extend `examples/benchmark.rs` with step page, cursor and summary reads and record them in `docs/performance/host-baseline.md`, recording the gap: actor streaming overhead, eight simultaneous streams and the 8 ms UI batch target remain unmeasured
- [x] 6.2 Update `docs/PRD.md` (Conversation section: activity card, history sheet and turn summary) and `docs/DEVELOPMENT.md` (step retention and bounds), and verify that the docs match the shipped behavior
- [x] 6.3 Add the follow-up "team view of child agents' live steps" to the PRD's later-phase list, and verify that it is recorded
