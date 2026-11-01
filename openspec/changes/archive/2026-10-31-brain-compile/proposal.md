## Why

Logs are what agents said they did. The wiki (`STATUS.md` and `notes/`) is the current truth that later sessions read first. Compiling turns the first into the second, and it has stalled: Sage's `STATUS.md` covers only the Klipy proxy from July, with 9 repository logs and 7 notes. Today it is a skill an agent runs by hand. Its cursor is a date marker that skips later entries from the same day, and it guesses where sibling repositories live in order to verify refs.

Michael decided on October 29, 2026 that there is one compiler per brain, recorded in the brain repository, and that a conflict ruling starts a compile right away. With per-entry files (`brain-log-capture`) and the PR flow (`brain-registry-sync`), Wiffletree can run compilation as a scheduled job: it runs on one machine, makes no model call when nothing is pending, and always goes through a compile PR.

## What Changes

- **One compiler per brain.** Only the host named in `wiffletree-brain.json` compiles that brain. Every other install only logs and reads.
- **Runs on #163's scheduler** (decided by Michael, October 29, 2026). Compile is a built-in, host-owned job kind of `assistant-spaces-jobs-boards`' scheduler, one per brain: a nightly run at the config's time and timezone, one catch-up run after sleep or downtime, Run now, and a run started when a ruling entry merges. At most one run and one open compile PR per brain at a time.
- **The session belongs to the brain.** A compile session is owned by the brain it compiles (`spaces`' session owners), not by a project or a Space, so several Spaces may name one brain and there is no hidden project record.
- **An entry-ID cursor stored with the wiki.** `notes/INDEX.md` records the commit the brain has been compiled through and the ids of later entries already processed. The cursor advances only when the compile PR merges, so a failed run loses nothing and same-day entries are never skipped.
- **Deterministic work before any model call.** The host finds the pending entries and verifies each entry's `ref` against the workspace's own repositories: commit, merged PR and release tag. It marks each entry verified, claimed or unverified. With no pending entries, the run makes zero model calls and writes nothing.
- **A model run under the bundled, versioned compile procedure.** It uses the auto-fold and queue rules of today's `brain-compile` skill:
  - non-conflicting observations and additive facts fold automatically;
  - contradictions of a `decision` note, disagreements between authors, and destructive or deprecating changes are queued;
  - claimed or unverified entries are never promoted to fact.
- **Conflicts become Inbox cards.** Each card shows the claim, the conflicting decision, the evidence and a recommended ruling: Uphold existing, Keep new, Keep both or Defer. A ruling writes a `decision` entry and starts a compile. `reconcile-queue.md` becomes an export of the open conflicts.
- **A compile PR changes only wiki paths:** `notes/**`, `STATUS.md` and `reconcile-queue.md`. The host checks this before pushing, and the gate's path-ownership check enforces it again.
- **A "Brain compiler" row in Who runs what**, optional like the PR keeper's, separate from #163's Job row. With no row, no compile runs and the Brains page says to choose one.

## Capabilities

### New Capabilities
- `brain-compile`: the compiler host, triggers and missed runs, the cursor, ref verification, the fold and queue rules, conflicts and rulings, the compile PR, run history and usage.

### Modified Capabilities
- `model-routing`: Who runs what gains an optional Brain compiler row, and a requirement for how that row is used. Delta order: 1 `assistant-spaces-jobs-boards` → 2 (this), which restates #163's rows.
- `workspace-host`: the enforced hierarchy admits compile sessions, owned by their brain. Delta order: 1 `spaces` → 2 `assistant-spaces-jobs-boards` → 3 (this).

## Impact

- Depends on `brain-registry-sync`, `brain-log-capture`, and `assistant-spaces-jobs-boards` stage 3 for the scheduler and its built-in host jobs.
- `crates/workspace-host/src/compile.rs` (new): the built-in compile job (its due-time rule and ruling trigger on #163's `jobs.rs`), cursor, pending set, ref verification, the compile session, the diff check and run records. It hands its branch to `brain-registry-sync`'s batch flow as a "compile run" batch.
- `crates/workspace-host/src/verify_refs.rs` (new): ref verification, shared with `brain-browser` for display.
- `crates/workspace-host/skills/procedures/compile.md` (new): the bundled, versioned compile procedure.
- `crates/workspace-host/src/mcp.rs`: `queue_conflict` and `finish_compile`, offered only to the compile session.
- `crates/workspace-host/src/policies.rs` and `selection.sql`: the Brain compiler row.
- `crates/workspace-desktop`: the Models page row; Run now and run history on the Brains page; conflict cards in the Inbox.
- Usage: compile runs count in the usage ledger under the brain.
- `docs/PRD.md`: Wiki and second brain, and Timed events and maintenance.
