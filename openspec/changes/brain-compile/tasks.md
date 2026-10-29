Depends on `brain-registry-sync` and `brain-log-capture`.

## 1. Routing

- [ ] 1.1 Add the optional Brain compiler row to Who runs what (`policies.rs`, `selection.sql`), with no skip and no small-work line. Tables saved before it load unchanged. Extend the table tests with the new refusals and the load case.
- [ ] 1.2 Models page: the Brain compiler row with No model.

## 2. Scheduling and the cursor

- [ ] 2.1 `compile.rs`: due times from the config's time and timezone, one catch-up run after missed times, Run now, and starting a run when a fetched `main` adds a `Ruling on conflict` entry. Only on the configured compiler host, with one run and one open compile PR per brain. Test sleep across a due time, a daylight-saving change, another host and an open PR.
- [ ] 2.2 Read and write the `compiled-through` / `compiled-after` cursor in `notes/INDEX.md`, compute the pending set with the 200-entry bound, and handle the first compile with the import's compiled ids. Test same-day entries, a late batch, a failed run and the bound.
- [ ] 2.3 The idle path: no pending entries means no model call and no write. Test that the row is not resolved.

## 3. Verification

- [ ] 3.1 `verify_refs.rs`: match by GitHub name, fetch once per run, classify verified, claimed or unverified with reasons, and record tag evidence. Test each `ref` form against fixture repositories.

## 4. The compile session

- [ ] 4.1 Bundle `skills/procedures/compile.md` with a version, restating the fold and queue rules. Build the bounded input (60,000 characters of notes).
- [ ] 4.2 Start a host-owned session on the Brain compiler row in a worktree, with the `queue_conflict` and `finish_compile` tools only and a 30-minute timeout. Record usage under the brain.
- [ ] 4.3 Check the result before pushing (paths, frontmatter, wikilinks, source ids), then write the cursor and `reconcile-queue.md` and hand the branch to the batch flow as a compile run. Test a change under `logs/`, a broken wikilink and a timeout, each leaving the cursor unchanged.

## 5. Conflicts and rulings

- [ ] 5.1 Record conflicts, export `reconcile-queue.md` with conflict markers, and read conflicts from `main` on every install. Show Inbox cards on each matching project and the Brains page fallback.
- [ ] 5.2 Rulings from the human's client only. Uphold existing, Keep new and Keep both write a decision entry with the `Ruling on conflict <id>:` state and open their batch at once; Defer hides the card until the next compile. Test each ruling and that agents cannot rule.

## 6. Runs and UI

- [ ] 6.1 Record runs and show the last and next run, Run now and the paginated history on the Brains page. Show "Brain compile · <brain>" in usage reports.

## 7. Docs and checks

- [ ] 7.1 Check that `docs/PRD.md`'s Wiki and second brain and Timed events sections match the implementation, and update them if not.
- [ ] 7.2 Run the compile, verification, routing and usage tests, then the full suite once before ready_for_testing.
