## Context

Today's `brain-compile` skill (in `personal-agents`) does the following:
- It reads pending entries above each log file's `<!-- compiled-through: YYYY-MM-DD -->` marker.
- It classifies refs as verified, claimed-only or unverifiable with `git cat-file` in sibling folders.
- It auto-applies everything except three kinds of conflict: a contradiction of a `kind: decision` note, two agents disagreeing on the same fact, and a destructive or deprecating change. It queues those in `reconcile-queue.md`.
- It rewrites `STATUS.md` (about 500 words) and the notes, which have `title`, `kind`, `updated` and `sources` frontmatter, and keeps `notes/INDEX.md` in sync.

It never auto-overwrites a decision note. A reversed decision becomes an observation marked superseded.

Wiffletree's current `memory.rs` only exports logs and records maintenance runs. Its spec says semantic compilation is unavailable. The `schedule` MCP tool sets a coordinator's own timers and is not a job scheduler. The PR keeper is the precedent for a model-backed host job with an optional row in Who runs what (`model-routing`, "The PR keeper's row").

After `brain-registry-sync` and `brain-log-capture`:
- every entry is one file under `logs/<repo>/YYYY/MM/`;
- the brain's compiler host is named in `wiffletree-brain.json` on `main`;
- changes reach `main` only through batch PRs.

The gate allows a compile PR to change only `second-brain/notes/**`, `second-brain/STATUS.md` and `second-brain/reconcile-queue.md`.

## Goals / Non-Goals

**Goals:**
- Pending entries become sourced notes and a current STATUS without a human running a skill.
- No entry is skipped or processed twice, across crashes, sleep and compiler moves.
- Idle brains cost nothing; a run with no pending entries makes no model call.
- Conflicts reach the human as decisions with evidence, and a ruling takes effect at once.

**Non-Goals:**
- Browsing and search (`brain-browser`).
- Archiving and pruning (the PRD's weekly job), which stays a later change.
- Letting the human edit the compile procedure on the Skills page. It is bundled and versioned; making it an editable stock skill can follow once it is stable.
- Compiling brains whose config names another host.

## Decisions

### Decided by Michael (October 29, 2026)

- One compiler per brain, named in the brain repository's config and changed through a PR.
- A ruling starts a compile right away.
- The ruling names are Uphold existing, Keep new, Keep both and Defer.
- Every change reaches `main` through a PR.

### Decided in this proposal, for review

1. **The cursor lives in `notes/INDEX.md`'s frontmatter**:
   ```yaml
   compiled-through: <commit on main>
   compiled-after: [<entry id>, …]
   ```
   - Pending entries are the files added under `logs/<repo>/` between `compiled-through` and `main`'s head, minus the ids in `compiled-after`.
   - A run processes at most 200 pending entries, oldest id first. When none are left before the run's head commit, the cursor advances `compiled-through` to that commit and clears `compiled-after`. Otherwise it adds the processed ids.
   - The cursor is an entry-ID cursor anchored on a commit. Storing it in the compile PR means it advances exactly when the notes do, and it travels with the brain if the compiler moves. Entries that merge late, from a batch opened earlier, are still found because they are added after `compiled-through`. A date cursor would miss them.
   - On a brain's first compile, with no cursor, every entry file is pending except the ids the legacy import recorded as already compiled. The legacy import is therefore offered only on the compiler host.
2. **Ref verification is deterministic and shared** (`verify_refs.rs`):
   - The entry's repository folder matches the workspace repositories with that GitHub name.
   - The host fetches each such repository at most once per run.
   - **verified:** a named commit is reachable from the repository's default branch, or the named PR is merged. A release tag containing the commit is shown as extra evidence.
   - **claimed:** a commit exists but is not on the default branch, or the PR is open.
   - **unverified:** the `ref` is `—` or `<branch> @ —`, the anchor is not found, or the repository is not on this machine.
   - The browser uses the same function to show verification on any install, so the result never has to be written into logs.
3. **The model run is a session owned by the brain** (decided by Michael, 2026-10-29: sessions outside projects belong to a Space or a brain).
   - Its owner is the brain it compiles (`SessionOwner::Brain`, from `spaces`), not a project or a Space, because several Spaces may name one brain. `workspace-host`'s "Enforced hierarchy" admits it, as the third delta after `spaces` and `assistant-spaces-jobs-boards`.
   - It uses the Brain compiler row, resolved like any agent start, and works in a worktree of the brain checkout on a compile batch branch.
   - Its input is bounded: the bundled procedure, STATUS, INDEX, the pending entries with their verification states, and the notes the entries name or link to, capped at 60,000 characters of notes.
   - It edits files only under the wiki paths. It reports conflicts with `queue_conflict` and ends with `finish_compile`. These two tools exist only for this session, and it has no other write tool.
   - The run has a timeout (30 minutes) and is recorded with its procedure version.
   - Its usage is recorded in the ledger under its owner, the brain, shown in usage reports as "Brain compile · <brain>". No project record holds it, so nothing has to be hidden from the sidebar.
4. **The fold and queue rules are today's skill rules**, restated in the bundled procedure:
   - Queue only a contradiction of a `kind: decision` note, a disagreement between two authors (or agents) about the same fact, or a destructive or deprecating change.
   - Fold everything else.
   - Claimed and unverified entries may be recorded as intentions or observations, never as completed fact, and never over a verified decision.
   - A decision note is changed only by a verified decision entry that does not contradict it, or by a ruling.
   - Notes keep the `title`, `kind`, `updated` and `sources` frontmatter, where `sources` lists entry ids.
   - Each claim cites its entries with a Markdown footnote, for example `…capped at 5 attempts.[^1]` with `[^1]: [2026-10-22](../logs/sagechat-call-watch-relay/2026/10/2026-10-22-01jbnk2w4y….md)`. Footnotes give the browser a Sources list per claim, as approved in the mockup. GitHub renders them as links, and the gate's markdownlint and wikilink checks accept them.
5. **The host checks the result before pushing:**
   - only wiki paths changed;
   - every note has the frontmatter;
   - every `[[wikilink]]` resolves;
   - every source id and footnote names an entry file that exists;
   - the INDEX cursor is the one the host computed. The host writes it itself, after the model finishes.

   A failed check ends the run as failed, with no PR and the cursor unchanged.
6. **Conflicts are recorded by the host** from `queue_conflict`, each with:
   - a stable id;
   - the rule it hit;
   - the claim (an entry id);
   - what it conflicts with (a note or an entry id);
   - evidence lines (verification states, PR states, the authors);
   - a recommended ruling with one line of reasoning.

   The host writes `reconcile-queue.md` from the open conflicts, one block each, marked `<!-- conflict:<id> -->`, in the same compile PR. Every install shows conflicts from `reconcile-queue.md` on `main`. A card appears in the Inbox of each project in a Space that names the brain whose repositories include the claim's repository. A conflict touching no project appears on the Brains page.
7. **Rulings:**
   - **Uphold existing:** a decision entry restating the established decision, marking the claim as not adopted.
   - **Keep new:** a decision entry adopting the claim; the old decision note becomes an observation marked superseded.
   - **Keep both:** the human writes one line scoping both, which becomes the decision entry.
   - **Defer:** no entry; the card hides until the next compile, which raises it again if it still holds.

   A ruling entry's author is the human, its repository is the claim's, and its `state` begins `Ruling on conflict <id>:`. The host opens its batch at once rather than waiting.
8. **A ruling starts a compile when its entry merges.** The compiler host starts a run whenever a fetched `main` adds an entry whose `state` begins `Ruling on conflict`. Rulings made on any install, by any teammate, therefore start a compile on the compiler. A ruling made on the compiler host also shows "Compile starts when the ruling merges".
9. **Missed runs and time come from #163's scheduler** (decided by Michael, 2026-10-29). Compile is a built-in host job of `assistant-spaces-jobs-boards`' scheduler ("Built-in host jobs"): one per registered brain, owned by the brain, due only on the compiler host. The scheduler supplies the rules below; this change adds the ruling trigger (decision 8), the open-PR wait (decision 10) and the idle path.
   - A run is due at the config's local time each day.
   - On start or wake, a host that missed one or more due times runs once.
   - Due times are computed in the config's timezone, so a daylight-saving change never yields two runs for one date.
   - Run now starts a run unless one is running or a compile PR is open, in which case it says why.
10. **One open compile PR per brain.** While one is open, scheduled runs are skipped and recorded as "waiting for PR #NN". A compile PR that fails a check raises an Inbox item on the Brains page through `brain-registry-sync`. The human can discard it, and the next run computes the pending set again from the unchanged cursor.
11. **The Brain compiler row** is optional, with no skip and no small-work line, and only the human's client changes it. It is its own row, not #163's Job row, because compiles write a brain while jobs only read (decided by Michael, 2026-10-29). Unlike the PR keeper's sorting, it may run on any provider. With no row, the compile job is held and the Brains page says to choose a model in Models.

### Options considered

- **The cursor in the host's store only.** This was rejected: it is lost when the compiler moves to another machine.
- **A cursor commit without an id list.** This was rejected: a run bounded to 200 entries could not advance past part of a commit.
- **Writing verification into logs.** This was rejected: log entries are append-only.
- **The coordinator's `schedule` timers.** This was rejected: they are a coordinator's own messages, not brain jobs, and a brain has no coordinator.

## Risks / Trade-offs

- **[Model output strays outside the rules]** → The host's diff checks and the gate catch path and link errors; the conflict rules depend on the model following the procedure. Mitigations: decision notes are never overwritten without a ruling, and every change is a reviewable PR with git history as undo.
- **[A big backlog after the legacy import]** → The 200-entry bound and the cursor's id list make the first compiles several PRs over consecutive runs. Run now can drain the backlog faster.
- **[Cost]** → Runs with nothing pending are free. Usage is visible per brain, and the Brain compiler row lets the human pick a cheaper model.
- **[The compiler host is offline for days]** → Logging continues; the backlog compiles on its next start. The Brains page shows the last run's age.

## Migration Plan

1. Ship with no Brain compiler row, so nothing runs.
2. Michael sets the row, and `wiffletree-brain.json` names his Mac (`brain-registry-sync`'s Set up).
3. Michael runs the legacy import (`brain-log-capture`), then Run now. The first runs drain the backlog through compile PRs.
4. The `sagehaven-docs` migration ticket retires the legacy files once a compile has folded them.

## Overlaps with other proposals

These are merged on `main`:
- PR #162 (`shared-issues`), as 6194b99a;
- PR #163 (`assistant-spaces-jobs-boards`), as 419a55ef;
- PR #164 (`create_project`), as a8dded2d.

#162's and #163's changes are open there, not yet archived; #164's is archived. `spaces`, carved out of #163 on October 29, 2026, ships before all of them. Where they change the same requirement as this change, this change goes after them: "Who runs what" is #163 → this change; "Enforced hierarchy" is `spaces` → #163 → this change.

**Alignment with #163's scheduled jobs — decided by Michael, 2026-10-29, as recommended here.** #163 already specifies:
- a host scheduler with schedules in an IANA timezone;
- daylight-saving handling: a skipped time runs at the next valid minute, and a repeated time runs once;
- missed runs coming back as one catch-up run;
- pause, resume and Run now;
- run history in the database.

That is exactly what decision 9 here re-specifies for compiles. Brain compiles can simply run on that mechanism: due times, catch-up, Run now and run history from #163's scheduler, with the compile as a built-in, host-owned job kind rather than a user recipe.

It stays a separate kind because #163's jobs:
- are user-owned recipes that no agent may create ("Only the user saves a job", "Jobs are not coordinator timers");
- run as read-only threads that only write files to the Space's storage ("Jobs only read from other apps").

A compile instead writes the brain through a gated PR, runs only on the brain's compiler host, starts after a ruling merges and makes no model call when nothing is pending.

So compiles reuse the scheduler and run records and keep the compile-specific rules here. "Compile runs start on schedule, on demand and after a ruling" now names compile as a built-in host job, and #163's scheduled-jobs spec gained "Built-in host jobs". Compiles keep their own Brain compiler row.

| Shared concept | Where else | Share or align | Open question |
| --- | --- | --- | --- |
| Optional rows in Who runs what | #162 no longer adds an Issue assistant row (its issue assistant became #163's issue threads); this change adds a Brain compiler row (any provider) | **Align** on the optional-row pattern, as with the PR keeper. | None. **Decided by Michael, 2026-10-29:** the table ends with Assistant, Router, Job and Brain compiler. |
| Running a model with narrow tools | Issue threads run with web off (#163); the compile session edits files in a worktree, so it needs file tools and keeps any provider | **Align** on tool-limited host sessions, but the needs differ. The compile session's limit is the host's diff check, not tools off. | Should the compile session also run tools-off, receiving notes as input and returning edits through a tool? That would be safer, but Claude only. |
| Inbox decision cards | Conflict cards here; #162 phase 5 paused-field and mention items; `quiet-findings` (archived) keeps noted findings out of the Inbox | **Align** the card shape: title, evidence, a recommended action and buttons, with one card per source. | None. |
| Untrusted text | #162 decision 10: issue text written by others is context, never instructions | **Align**: log entries are written by agents and teammates, so the compile procedure treats entry text as data. The conflict tools accept only structured fields. | Should the fixed-core wording for untrusted text be shared between issues and brain content? |
| Auto-applied state from evidence | #162 derives issue states from linked PRs; here refs are verified against repositories and PRs | **Share** the PR read path (`pr-watch` and `verify_refs`) where possible. | None. |
| The gate on compile PRs | `brain-gate`'s path ownership and wikilink checks | **Share**: the host's pre-push checks mirror the gate's so failures surface before a PR. | Should the host run the gate's checks locally from `gate/` before pushing, instead of re-implementing them? This is recommended once `brain-gate` ships. |
| Scheduling, catch-up and Run now | #163 (merged): scheduled jobs with timezones, daylight-saving rules, one catch-up run, Run now, pause and run history | **Share** the scheduler and run records (alignment above); keep the compile-specific rules. | None. **Decided by Michael, 2026-10-29:** compiles run on #163's scheduler as a built-in, host-owned job kind. |
| Who runs what | #163 adds Assistant, Router and an optional Job row; this change adds Brain compiler | **Align**: this change goes after #163 and restates its rows in its "Who runs what" delta (task 1.1). | None. **Decided by Michael, 2026-10-29:** compiles keep their own Brain compiler row, not the Job row. |
| A session outside any project | `spaces` gives sessions outside projects an owner, a project, a Space or a brain; #163 admits threads, issue threads and job runs owned by a Space; #162 first proposed a hidden project record of kind `tracker`, now dropped | **Share** the owner type: the compile session is owned by the brain it compiles (decision 3), so several Spaces naming one brain is no problem. | None. **Decided by Michael, 2026-10-29:** sessions outside projects belong to a Space or a brain; compile belongs to the brain, and there are no hidden project records. |
| Provider restrictions | #163 runs threads and job runs outside YOLO, with tools limited by the provider | **Align**: a compile session needs file writes in its worktree only; #163's Claude allow-list pattern (writes limited to one folder) fits it. | Should the compile session use #163's restricted launch, with writes limited to its worktree's wiki paths? |
| Projects created by a coordinator | #164 (merged) | None: compiles are per brain, not per project. | None. |

## Open Questions

None blocking.
