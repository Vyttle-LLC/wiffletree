Depends on `brain-registry-sync`.

## 1. Store and migration

- [x] 1.1 Extend `LogInput` and `WorkLog` (`crates/workspace-core/src/lib.rs`): optional milestone and repository, the entry id, brain, folder, file path, batch, author, provider, model and machine. Add `log_batches`.
- [x] 1.2 Migration: keep every `work_logs` row. Mark exported rows as "in legacy file", queue unexported rows when their project's Space has a brain, then drop `log_exports` and `maintenance_runs`. Test both kinds of row and a project without a brain.

## 2. Capture

- [x] 2.1 `memory.rs`: rework `append_log` into the shared capture path. Resolve the brain from the caller's Space (project or assistant thread), mint the ULID and date, compute `ref` from the ticket's worktree, read the gh login, record provider, model and machine, and keep milestone deduplication. Test each `ref` form, the refusal cases and deduplication.
- [x] 2.2 Per-entry renderer and the 2,000-character cap. Test rendered entries against the gate's good and bad entry fixtures in `gate/tests/fixtures/` (`brain-gate`). Do not copy them. Remove `render_log`, `insert_entries`, `export_logs`, `export_batch` and the lock file.
- [x] 2.3 `mcp.rs`: the `log_work` tool for the coordinator and implementer only. Test the tool lists per role and that `ref`, `author` and `id` cannot be supplied.

- [ ] 2.4 Give assistant threads `log_work` (`assistant-spaces-jobs-boards` thread tools): entries in `logs/assistant/YYYY/MM/` with `repo: assistant` and `ref: —`, thread id and Space in the store, never `logs/assistant.md`. Job runs get no `log_work`. Test a thread entry against the gate fixtures.
- [ ] 2.5 Before archiving, check that `spaces` is archived, so that this change's REMOVED "Safe Markdown export" removes `spaces`' version.

## 3. Batches and prompts

- [x] 3.1 Batching per brain and ticket, coordinator session or assistant thread. Close a batch on accept, close, archive, 24 hours or Send now, capped at 50 entries. Hand closed batches to `brain-registry-sync`. Test one PR per ticket, the 24-hour flush and the cap. (The assistant thread's batch and its close on thread deletion come with 2.4.)
- [x] 3.2 Milestone prompts in the results of `report` (ready_for_testing), `accept_ticket` and `publish_ticket`, once per milestone and only with a brain. Test prompted, already-logged and no-brain cases.

## 4. Legacy import

- [ ] 4.1 Parse legacy `logs/<repo>.md`. Derive deterministic ids, find authors by blame then `gh api` with the importer as fallback, fit the cap, map refs and record compiled markers. Write log-only PRs of at most 200 files. Test idempotence, the oversize case, a malformed ref and the compiled marker.
- [ ] 4.2 The Import legacy logs action on the Brains page, offered until the import is done.

## 5. Instructions and desktop

- [ ] 5.1 Add the log rules to `skills/core/coordinator.md` and `implementer.md`, and bump their versions. Assert the wording in `crates/workspace-host/tests/contracts.rs`.
- [ ] 5.2 `memory_view.rs`: the capture form writes through the shared path, unsent rows are listed, and the export button is removed.

## 6. Docs and checks

- [ ] 6.1 Check that `docs/PRD.md`'s Work logs section matches the implementation, and update it if not.
- [ ] 6.2 Run the memory, contracts and migration tests, then the full suite once before ready_for_testing.
