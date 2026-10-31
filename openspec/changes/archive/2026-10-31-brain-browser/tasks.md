Depends on `brain-registry-sync` and `brain-log-capture`. Status and Conflicts become complete once `brain-compile` ships.

## 1. Index

- [x] 1.1 `brain_index.sql` and `brain_index.rs`: per-brain FTS5 (`unicode61`, with `_`, `-` and `/` as separators) and the metadata table. Reindex incrementally by hash after each fetch, and rebuild when the index is missing. Test the changed-file reindex, deletes and rebuild equality.
- [x] 1.2 Read API: paginated Logs by keyset with filters, the note tree from INDEX headings, rendered notes, backlinks, footnote sources joined with `verify_refs`, conflicts from `reconcile-queue.md`, and the project scope. Test the scope rules and the hidden-notes count.
- [x] 1.3 Search ranking: BM25 with decisions above observations, `snippet()` excerpts and a prefix query for the last term.
- [x] 1.4 Performance fixture: 100,000 entries and 10,000 notes. Assert the first Logs page and the first search page within the budget from cache.

## 2. query_brain

- [x] 2.1 `mcp.rs`: `query_brain` for the coordinator, implementers, testers and reviewers. Threads, issue threads and job runs get it with `assistant-spaces-jobs-boards` (its task 8.1, moved there). Resolve the brain and refuse as specified, walk the ladder with the 4,000-character budget, and report freshness and "nothing on this". Test each refusal, the ladder order, a cross-organization attempt and the empty case.
- [x] 2.2 Mention `query_brain` in the fixed core instructions, and assert it in `contracts.rs`.

## 3. Desktop

- [x] 3.1 The brain browser page: scope chips, the brain switcher and the Status, Wiki, Logs and Conflicts views, following `design/mockups/current/brains.html`. Notes render as Markdown with wikilinks, footnotes and backlinks.
- [x] 3.2 Logs entries show their PR stage, ticket and conversation links, pending entries, and the "logged outside this install" note.
- [x] 3.3 `memory_view.rs`: the project summary (brain, batches, conflicts link, recent entries, unsent rows, capture form, Open brain browser, Search).
- [x] 3.4 The search overlay: ⌘K, Ctrl+K and `/`; arrows, Enter, Tab and Esc with focus return; the brain resolved from the project's Space (a thread's comes with `assistant-spaces-jobs-boards` task 8.1). Check it by keyboard only.

## 4. Docs and checks

- [x] 4.1 Check that `docs/PRD.md`'s Memory interface and performance section matches the implementation, and update it if not.
- [x] 4.2 Run the index, MCP and desktop tests and the performance fixture, then the full suite once before ready_for_testing.
