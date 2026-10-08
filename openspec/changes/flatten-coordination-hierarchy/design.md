## Context

Today's hierarchy is main coordinator (`Role::ProjectOrchestrator`, label "Main coordinator") → repository coordinator (`Role::TaskOrchestrator`, label "Repository coordinator") → tickets → agents (`Implementer`, `Tester`, `Reviewer`).

- `create_repo_coordinator` (`live.rs`, `agent_tool`) creates one `task_orchestrator` session per repository with `repository_id` set.
- `create_ticket` requires a `task_orchestrator` caller and reads the repository from that session. `worktrees.rs::ticket_repository` does the same for every worktree operation.
- `assign_ticket`, `accept_ticket` and `close_ticket` check `ticket.coordinator_id == caller`. `archive_team` lets the main coordinator archive a whole repository team, closing its accepted tickets.
- Worker sessions are children of the repository coordinator (`sessions.parent_id`), so every report wakes it, and it relays to the main coordinator.
- `report` kinds are `progress`, `blocked`, `ready_for_testing`, `passed`, `failed` and `completed`. A non-progress report sets `ticket.state` to the kind and wakes the parent. Only progress is batched (`IS_PROGRESS_REPORT` in `service.rs`).
- Tables involved: `sessions`, `tickets (id, coordinator_id, data)`, `runtimes` (`SessionRuntime.ticket_id`, `focus`, `workdir`), `messages`, `provider_runs (id, session_id, messages, started_at, finished_at, outcome, detail, process_group)`, `pending_worktree_removals` and `schedules`. The store is at `user_version = 5`.
- Worktrees live at `<workspaces_dir>/tasks/<project>/<repository>/<ticket>` on branch `wiffletree/<ticket>` (`settings.rs::new_ticket_place`).
- #19 made accept, close and archive remove the worktree. Tickets finished before it kept theirs.
- Role contracts are `skills/main-coordinator.md`, `skills/repository-coordinator.md`, `skills/communication.md`, `skills/implementer.md` and `skills/tester.md` (tester and reviewer share one contract).
- The project's concurrent turn limit is `Project.turn_limit` (4 by default), enforced in `service.rs::schedule`.

## Goals / Non-Goals

**Goals:**
- Two levels: the project coordinator talks to ticket agents directly; no relay turns.
- A ticket is the workspace: one repository, one worktree and branch, its agents beneath it.
- One call verifies a ticket with every configured verifier, concurrently, pinned to one commit, with a capped fix loop that needs no coordinator turn per round.
- Migrate existing stores without deleting anything.
- A safe, approval-gated cleanup of pre-#19 leftover worktrees.

**Non-Goals:**
- Any change to model selection: `provider`, `size`, `complexity` and `profile` arguments, Big/Small profiles, role policies and the Models view stay as they are. The Model guide project owns them.
- Merging, pushing, PRs or a managed integration branch (PRD's later "Workflow and integration" stage).
- Cross-repository contracts beyond what the project coordinator already owns.
- Changing worktree paths, branch naming or #19's removal rules.

## Decisions

### 1. The project coordinator owns tickets; `task_orchestrator` becomes legacy
`Ticket` gains `repository_id`. `tickets.coordinator_id` holds the project coordinator. `create_ticket` takes `repository_id`, checks `Project::uses`, and uses that repository's base. `ticket_repository` reads `ticket.repository_id`. Agents are created with the project coordinator as parent and the ticket's repository as `repository_id`, so the existing parent/child rule for `send_message` and `report` connects them directly.

`Role::TaskOrchestrator` stays in the enum only so archived sessions decode and display. Creating it is refused everywhere (`CreateSession`, MCP, desktop). Removing the variant would break decoding of migrated rows and buys nothing. *Alternative considered:* rewrite migrated rows to another role. Rejected: it falsifies history.

### 2. Tool surface

| Tool | Change |
| --- | --- |
| `create_repo_coordinator` | **Removed.** |
| `archive_team` | **Removed.** With no teams there is nothing to archive; `close_ticket` already archives a ticket's agents, and this change makes `accept_ticket` do the same (today it only removes the worktree). Archiving a whole project stays a human action. |
| `create_ticket` | Caller must be the project coordinator. New required `repository_id`. Idempotent per (coordinator, repository, title). |
| `assign_ticket` | Caller must own the ticket (now the project coordinator). Model-selection arguments unchanged. Still used for implementers and for ad-hoc testers or reviewers outside a verification cycle. |
| `verify_ticket` | **Added.** `{ticket_id}`. Starts a verification cycle (decision 4). Returns the cycle record. |
| `accept_ticket` | Caller must own the ticket. Requires `passed` and HEAD equal to the last verified commit. Now also archives the ticket's agents, like `close_ticket`, which makes `archive_team` unnecessary. |
| `close_ticket` | Caller must own the ticket. Otherwise unchanged. |
| `report` | Kinds unchanged. While a cycle runs, verifier and implementer reports are routed by the cycle (decision 5) and batched to the coordinator instead of waking it. `passed` is still limited to testers and reviewers. |
| `workspace_context` | `teams` is replaced by the project coordinator's tickets, each with repository, state, agents and verification record. |
| `send_message`, `schedule`, `unschedule`, `list_schedules`, `ask_user`, `close_question`, `request_permission` | Unchanged, except that tool descriptions say "Coordinator" instead of "Main coordinator" or "Repository coordinator". |

Desktop commands: `Command::CreateTicket` gains `repository_id`, and its `coordinator_id` must be a project coordinator. `Command::CreateSession` refuses `task_orchestrator`. New `Command::LeftoverWorktrees` and `Command::RemoveLeftoverWorktrees { ticket_ids }` (decision 8).

### 3. Role contracts
- `skills/repository-coordinator.md` is deleted.
- `skills/main-coordinator.md` becomes the project coordinator contract, v4. It keeps the planning, `ask_user`, `close_question`, evidence and timer paragraphs. It replaces the repository-team paragraphs with ticket rules: create one ticket per repository-bounded unit with acceptance criteria; assign one implementer per ticket using the existing profile rules (text carried over from the repository coordinator contract unchanged); call `verify_ticket` when the implementer reports ready; act on the cycle's single outcome; and accept or close.
- `skills/communication.md`, v4: "the repository coordinator accepts the ticket" becomes "the coordinator accepts the ticket". "Repository coordinators and workers escalate to their parent" becomes "workers escalate to the coordinator".
- `skills/implementer.md`, v2: verification failures arrive as one message per round. Commit fixes, then report `ready_for_testing` again; do not argue with or message verifiers.
- `skills/tester.md`, v3 (tester and reviewer): verify the pinned commit named in the message; never edit, stage, commit or switch branches (now enforced, decision 6); the coordinator owns acceptance.

### 4. Verification configuration and cycle
**Configuration.** It lives workspace-wide in host `settings.json` beside `workspaces_dir`, as `verification: { verifiers: [{role, focus, instruction?, provider?, size?}], max_rounds: 2 }`. Only `provider` and `size` are accepted, not `complexity` or `profile`; they are passed through the existing assignment routing (`assignment_route`) untouched. It is edited in Settings. This is the simplest scope that serves the example (Claude tester, Codex tester, Reviso style reviewer as a `reviewer` with focus "Style" and an instruction to run the Reviso style review). Per-project or per-repository overrides are not built (decision Q1 below).

**Cycle record.** `Ticket` gains `verification`:
`{ cycle, max_rounds, outcome: running|passed|blocked, rounds: [{ round, commit, verifiers: [{ session_id, focus, message_id, result: pending|passed|failed|blocked, reason? }] }] }`

It is stored in `tickets.data`, which needs no new table. `max_rounds` is copied from settings at cycle start, so editing settings mid-cycle does not change a running cycle.

**Start.** `verify_ticket` checks, in order:
1. Ownership.
2. State is `ready_for_testing`, or, for a ticket migrated without a verification record, `passed` or `failed` (decision 7).
3. No running cycle.
4. `git status --porcelain` in the worktree is empty.
5. Verifier count ≤ min(`turn_limit - 1`, host worker limit). `turn_limit - 1` is the worker share of the project's turns (`service.rs` reserves one turn for coordinators). The host worker limit is the host-wide cap on active turns that workers respect, today the literal `6` in `service.rs::schedule`; this change extracts it as the named constant `HOST_WORKER_TURNS`, and both the scheduler and this check use it.

It then pins `git rev-parse HEAD`. For each verifier it reuses the (role, focus) session through `assign_ticket`'s idempotency or creates it, and sends `verify:{ticket}:{cycle}:{round}:{session}` with the coordinator as sender, naming the commit and the verifier's instruction. Finally it sets state `verifying`.

**Concurrency.** Today `service.rs::schedule` lets only reviewers share a ticket's worktree. Testers are treated as writers and serialize, and workers get at most `turn_limit - 1` project turns and `HOST_WORKER_TURNS` host-wide. Two changes make a round run together:
- Every tester and reviewer in a running cycle counts as a reader. The implementer has no due input during a round, so it never contends.
- A round is admitted all-or-none. If not every verifier of the round fits under the project and host-wide limits, none start until all fit.

The messages are queued in one transaction, so a single scheduling pass sees them together. The precondition above guarantees the round can fit under both limits. While a round waits, the scheduler starts no new worker turn host-wide (any project), so freed slots accumulate until the round fits.

**Proof of overlap.** It measures each verifier's first turn of the round, the one that consumed its `verify:` message. It uses only stored fields: for round *r*, take each `(session_id, message_id)` from `tickets.data.verification.rounds[r].verifiers` and select the `provider_runs` row with that `session_id` whose `messages` JSON array contains `message_id`. The round overlapped iff `MAX(started_at) < MIN(finished_at)`. Both are millisecond epoch values written by `record_turn_start` and the run's finish.

### 5. Report routing during a cycle
While `verification.outcome == running`, the generic rule in `report` that sets `ticket.state` to the report's kind (`live.rs`) is bypassed for the ticket's verifiers and implementer. The cycle alone sets the state: `verifying` when a round starts (round 1 and every re-run round) and `passed`, `failed` or `blocked` when a round ends. A verifier's early `passed` therefore leaves the ticket `verifying`, and `accept_ticket`, which requires `passed`, refuses mid-round.

Only verifier verdicts and the implementer's `ready_for_testing` are routed by the cycle. Any other non-progress implementer report (for example `blocked` while fixing) ends the cycle at once: `outcome = blocked`, ticket `blocked`, and the report wakes the coordinator immediately. A verifier verdict that arrives after its cycle has ended, for example after the implementer reported `blocked`, is recorded on its round and stored for the coordinator's next batch, but it changes neither the ticket's state nor the cycle's outcome.

**Which input a verdict answers.** Verifier sessions are reused across rounds and cycles, so a verdict is tied to the round input it answers: the verifier's latest `verify:` message on the ticket that a turn has taken (receipt delivered, acknowledged, completed or held). A verdict counts for the current round only when that message is the verifier's message in the current round and its result is still pending. The ticket's verifiers include every session that ever took one of its `verify:` messages, so a verifier dropped from the settings between cycles is still recognized. Every other verdict from one of the ticket's verifiers is late: for example a turn that took cycle 1's input reporting after cycle 2 started, or a second verdict after the cycle passed. A late verdict is stored quietly for the coordinator's next turn and changes neither the ticket's state nor any cycle.

- **Verifier `passed`, `failed` or `blocked`.** The host records the result on the current round, after the read-only check in decision 6. The message to the coordinator is still stored, but it is *quiet*: it rides along with the coordinator's next turn and never starts one.
- **Round end** (no `pending` result left):
  - Every result in the cycle's latest results is passed → `outcome = passed`, ticket `passed`, and one waking summary to the coordinator.
  - Any `blocked` → `outcome = blocked`, ticket `blocked`, and one waking message to the coordinator with the reports.
  - Any `failed` and `round < max_rounds` → ticket `failed`, and one message `verification:{ticket}:{cycle}:{round}` to the implementer session that last reported ready, listing each failed verifier's report.
  - Any `failed` and `round == max_rounds` → `outcome = blocked`, ticket `blocked`, and one waking message to the coordinator listing all unresolved failures, the rounds used and the cap.
- **Implementer `ready_for_testing`.** The report fails as a tool error if the worktree is dirty. Otherwise it is batched to the coordinator, and the host starts round + 1 with only the verifiers whose latest result is `failed`, pinned to the new HEAD, setting the ticket to `verifying` directly (never `ready_for_testing`).

The cap is a status, not an escalation: the coordinator decides whether to fix further, call `verify_ticket` again (a new cycle with every verifier), close the ticket, or `ask_user`. This keeps the project rule that a blocked worker does not automatically need the human.

**Quiet messages.** Schema 6 adds `messages.quiet` (default 0). The scheduler never starts a turn for a quiet message, but `due_input` delivers queued quiet messages with whatever next wakes the recipient. Progress batching was not enough: a batched progress report still starts the parent's turn once its 20-second window closes, so a round's verdicts would wake the coordinator and break the "no coordinator turn" scenario. Deciding by the sender's role at scheduling time was not enough either, because a verdict that arrives after its cycle ended must still not wake anyone. Verifier verdicts, the implementer's `ready_for_testing` during a cycle and the migration notice are quiet. The cycle's outcome message and every other report still wake the coordinator.

### 6. Read-only enforcement
Prompting alone is not enough, so the host checks. When a verifier reports `passed` or `failed`, it compares worktree HEAD to the round's commit and requires a clean `git status --porcelain`. If either fails, that verifier's result becomes `failed` with "worktree changed during verification". The changed worktree then blocks `accept_ticket` through the HEAD check. Ignored files such as build output do not count, matching #19's `check_removable`.

### 7. Migration (store `user_version` 5 → 6)
This runs in `Host::open` after the existing migrations and before `recover` schedules anything.

1. Copy `workspace.sqlite3` to `workspace.sqlite3.pre-flatten` (WAL checkpoint first), unless the copy already exists.
2. `BEGIN IMMEDIATE`. For each `task_orchestrator` session R with parent M:
   - Set each ticket's `coordinator_id` to M (column and JSON) and its `repository_id` to R's.
   - Set each child session's `parent_id` to M (column and JSON).
   - Cancel R's queued or held inbound messages. Re-send those from R's children to M as `migrated:{original_id}`, prefixed with the original id. List those from M in the notice.
   - Stop R's schedules (`stopped = 'migrated'`).
   - Resolve R's open permission attention as denied with reason "coordinator migrated".
   - Archive R.
   - Record a `team_migrated` activity event.
   - Queue one notice to M as a batched (non-waking) message `migration:{R}`.
3. Set `PRAGMA user_version = 6`, then `COMMIT`. A crash before commit leaves version 5 and the whole migration re-runs. The backup step is skipped when the copy exists.

The version guard `ensure!(version <= 5)` becomes `<= 6`, so an older app refuses a migrated store instead of misreading it. `change_archived(false)` refuses `task_orchestrator` sessions, so they stay read-only history. Archived repository coordinators are migrated too, so their tickets show under the coordinator's archived history.

**In-flight tickets** (decided, Q3): migrate immediately. At host start no turn is running. Interrupted turns already come back held for Retry or Skip (`recover_unfinished_turns`), worktrees and branches are untouched, and agents keep their provider conversations. Their next report simply reaches M. A ticket migrated as `passed` or `failed` has no verification record, so `accept_ticket` refuses it; `verify_ticket` accepts such a ticket (after the ownership and clean-worktree checks) and starts its first cycle. There is no bypass in `accept_ticket`. Unread instructions from M to R are cancelled and listed in M's notice; they are not re-sent to any agent, so M decides what still applies.

### 8. Leftover worktree cleanup
**Identification** (`Command::LeftoverWorktrees`, read-only). A ticket is a leftover when its state is `accepted` or `closed`, or all its agents are archived, `Path::new(&ticket.worktree).exists()`, and it has no `pending_worktree_removals` row. The listing also scans `git worktree list --porcelain` of every workspace repository and adds worktrees under `<workspaces_dir>/tasks/` on `refs/heads/wiffletree/*` that no ticket records, as `untracked_by_wiffletree`.

**Classification**, first match wins. Each class reuses existing checks where they exist:

| Class | Check | Removable |
| --- | --- | --- |
| `in_turn` | `agent_in_turn` | No |
| `locked` | `registered_worktrees(..).locked` | No |
| `dirty` | `git status --porcelain` non-empty | No |
| `detached` | worktree HEAD ref ≠ `refs/heads/<branch>` and `git merge-base --is-ancestor HEAD <branch>` fails | No |
| `untracked_by_wiffletree` | no ticket row | No |
| `unpushed` | `git rev-list --count <branch> --not --remotes` > 0 | Only if individually selected; branch kept |
| `clean` | none of the above | Only if individually selected |

**Flow.** The Repositories view gains "Leftover worktrees…". It opens a sheet showing the dry-run list grouped by project, with checkboxes only on `clean` and `unpushed` rows, none pre-checked. "Remove selected" asks for confirmation naming the count. `Command::RemoveLeftoverWorktrees` re-classifies each id, skips anything that changed class, and runs `git worktree remove` without `--force`. It never deletes branches or rows, records `worktree_removed` or `worktree_kept` activity per ticket, and returns per-row outcomes. No agent tool can trigger removal.

### 9. Sidebar and inspector
`sidebar.rs::tree` drops the team level: project row (coordinator) → one row per ticket of that coordinator → that ticket's agents (matched through `runtime.ticket_id`, as today). `ticket_row` shows `"{repository.name} · {ticket.title}"` and the state color. The "Older stores can contain workers created before ticket ownership existed" branch stays, under the coordinator. In `creation.rs`, `Creation::Coordinator` ("Add repository team") is removed, and `Creation::Ticket` gains a repository picker limited to the project's repositories. In `team_view.rs` and `inspector.rs`, the coordinator's "Teams" destination becomes "Tickets" and lists tickets with repository, state, agents and verification rounds. Selecting a ticket opens it there.

### 10. Documentation
These are tasks, not edits in this change, so docs never describe unshipped behavior:
- `openspec/config.yaml` hierarchy line → `Hierarchy: project coordinator -> tickets (one repository worktree each) -> agents.`
- `AGENTS.md` → "Keep project → ticket → agents semantics…".
- `docs/PRD.md` sections "Product principles", "Project hierarchy and ownership", "Interface requirements", reviewers and "Agent communication and shared context", Usage, and the delivery stages.
- `README.md` ("Each repository gets its own team lead").
- `design/README.md` line 7.
- `docs/DEVELOPMENT.md` (Teams, archive).

The historical mockups stay intact.

## Risks / Trade-offs

- **Coordinator context grows** because it sees every ticket's reports directly. → Verification rounds are batched and only cycle outcomes wake it. The existing turn budget and progress batching still apply.
- **Passed verifiers do not re-check later commits.** Only failed verifiers re-run, as requested, so a fix could regress what an earlier verifier passed. → The final summary names the commit each verifier passed. The coordinator can start a fresh full cycle before accepting.
- **All-or-none admission can starve a round.** Other workers anywhere on the host can keep taking freed slots one at a time, so the round never finds all its slots free at once. → While a round is waiting, the scheduler admits no new worker turn host-wide, in any project, until the round has started; coordinators still get their reserved turn. The verifier-count precondition (≤ min(`turn_limit - 1`, `HOST_WORKER_TURNS`)) guarantees the round eventually fits. Verifier rounds are short and rare, so the brief hold on other workers is accepted.
- **Turn limit couples to verifier count.** → `verify_ticket` refuses with both numbers rather than silently serializing. Raising `turn_limit` is a project setting.
- **Loss of per-repository coordinator memory** across tickets. → Ticket briefs and project memory carry context. The PRD's future integration-branch stage will need its own design without a standing coordinator.
- **Migration rewrites ownership columns.** → One transaction, a version gate, a kept file backup, no deleted rows and no rewritten message bodies.
- **Cleanup removes worktrees with unpushed branches.** → Branches are never deleted, so commits survive. Such rows are unselected by default and labelled.

## Migration Plan

1. Ship code, migration and docs together.
2. On first start the store backs up and migrates (decision 7). Each coordinator gets its notice on its next turn.
3. The human optionally opens "Leftover worktrees…" and removes approved entries.
4. Rollback: restore `workspace.sqlite3.pre-flatten` with the previous app version. Worktrees and branches are unaffected by the migration itself.

## Decisions on the open questions

The human and the coordinator settled every open question before implementation:

1. **Verifier configuration scope.** Workspace-wide, in host `settings.json` (decision 4). No per-project or per-repository override.
2. **Multi-repository features.** One ticket per repository under the project coordinator, with cross-repository contracts in each brief. No grouping or dependency field.
3. **In-flight tickets at migration.** Migrate immediately at startup. Unread worker reports to a repository coordinator are re-sent to the project coordinator. Unread instructions from the project coordinator to its repository coordinator are listed in the migration notice but not re-sent.
4. **Automatic verification.** Keep the explicit `verify_ticket` call.
5. **Accept archives agents.** `accept_ticket` archives the ticket's agents, and `archive_team` is removed.
6. **Unpushed leftover worktrees.** Removable only when individually selected. Branches are always kept.
7. **Stale verifier passes.** Only failed verifiers re-run.
8. **Unused `task_orchestrator` policy.** Its policy row stays untouched; the Model guide project owns it.

The host-wide hold on new worker turns while a round waits for capacity is approved.
