## Context

See proposal.md for why. The current flow, on origin/main 04debd9:

- `workspace-core/src/lib.rs`: `VerificationSettings` (verifiers, `max_rounds`, default 2) and its `Default` with focuses "Tests", "Claude" and "Codex". `Ticket.verification` is `Option<Verification>`, the latest cycle only. `VerifierRun.reason` already records a host override of a reported result. Tickets are stored as JSON (`tickets.data`), so new fields with serde defaults need no SQL migration.
- `workspace-host/src/verification.rs`: `verify_ticket` numbers cycles without a cap (`:139`) and overwrites the previous cycle (`:188`). `start_round` builds the verifier message: brief, focus, worktree and branch, with no base or diff, and with the configured instruction only in round 1 because `next_round` passes `Default::default()` (`:602`). `report` routes verdicts quietly. `record_verdict` maps the reported kind to a result. `end_round` sends every failed report to the implementer with the coordinator as sender (`:503`). The cycle-outcome message goes to the coordinator from the host (sender `None`, `:564`), and the passed text ends "Accept the ticket or start a fresh cycle" (`:548`).
- `workspace-host/src/live.rs`: `accept_ticket` (`:386`) requires state `passed`, a passed cycle and HEAD equal to the verified commit. `finish_ticket` records `ticket_accepted` with the ticket id as its body. `agent_tool` dispatches tools, and `ticket_overview` serializes each `Ticket` into `workspace_context`.
- `workspace-host/src/mcp.rs`: tool schemas for `report`, `verify_ticket` and `accept_ticket`.
- Skills: `implementer.md` v3 ("Do not argue with or message the verifiers"), `main-coordinator.md` v8 ("call verify_ticket again for a fresh cycle with every verifier"), `tester.md` v4.
- Desktop: `sidebar.rs::ticket_row` draws a state dot from `ui.rs::ticket_state_color`. `team_view.rs::tickets` shows `tree::verification_lines` on each ticket card. `verifiers.rs` edits the round cap.

## Goals / Non-Goals

**Goals:** a deterministic verdict from structured findings; one ledger per ticket that the coordinator, the human and later verifiers all read; a hard end to every ticket's verification; acceptance that survives a squash or clean rebase.

**Non-Goals:** judging evidence quality in the host, which only checks that the fields are present; new storage tables; the diff-growth alarm; the PR watcher; changes to the open-pr skill, which lives outside this repository; per-reviewer statistics. Keeping findings makes those statistics possible later.

## Decisions

### 1. Findings ride on `report`; no new verifier tool
`report` gains an optional `findings` array. Each item has `severity` (`blocking` | `non_blocking` | `pre_existing`), `location` (`path:line`), `summary` (one line), `trigger`, `evidence`, and an optional `id` that re-reports an existing open entry. Findings are recorded only when the report is a verdict for the verifier's current round (`Routing::Verdict`). In any other report they are ignored, and the body still carries the text. Length bounds match the other text fields. A malformed item, or an `id` that is not an open entry of the reporting verifier's focus, refuses the whole report so the verifier can correct it.
*Alternative:* a separate `report_findings` tool. Rejected because it adds a second call that has to be ordered with the verdict.

### 2. The host computes the result
In `record_verdict`, after the existing worktree check:

- `blocked` stays `Blocked`.
- A changed worktree stays `Failed` with `WORKTREE_CHANGED`.
- Otherwise the result is `Failed` if the report contains an **evidenced blocking** finding, and `Passed` if not. An evidenced blocking finding has severity `blocking`, a `location` ending in `:<line>`, and a non-empty `trigger` and `evidence`.

When this differs from the reported kind, `VerifierRun.reason` says why ("reported failed without an evidenced blocking finding", or the reverse). A blocking finding without evidence fails nothing; it enters the ledger as `untriaged`.
*Alternative:* refuse a `failed` report that has no evidenced blocking finding. Rejected because a refusal can leave a verifier's turn without any verdict, and the override field already exists.

### 3. The ledger lives on the ticket
`Ticket.ledger: Vec<LedgerEntry>`. Each entry has an `id` (`F1`, `F2`, … per ticket), the finding's fields, its source (`focus`, `cycle`, `round`), a `status`, and a `reason` for a coordinator decision.

`status` is one of `open`, `fixed`, `untriaged`, `fix_now`, `follow_up` or `wont_fix`.

- `record_verdict` appends each new finding: evidenced blocking findings as `open`, all others as `untriaged`. A re-reported `id` keeps its entry open and updates its round and evidence.
- When a verifier reports a verdict, each of its focus's other `open` entries that it did not re-report becomes `fixed`. This covers round 2 and later rounds, and a new cycle's first round.
- When a cycle ends, an `open` entry whose focus had no verifier in that cycle becomes `untriaged`, so a renamed or removed verifier never leaves an entry nobody can close.
- Ledger entries are never deleted.

*Alternative:* store findings on `VerifierRun` and derive the ledger from them. Rejected because decisions and the fixed status would still need a home keyed by finding, which is two structures for one fact.

### 4. Routing: only open entries, sent as the host
`end_round` replaces the per-verifier report dump with the round's `open` entries (id, location, summary, trigger, evidence). It sends them with sender `None` and keeps the existing message id `verification:{ticket}:{cycle}:{round}`. The message tells the implementer to fix each one with the smallest change, or to object to the coordinator in one sentence with evidence. As today, only failed verifiers re-run, and passed ones retire. `findings()` stays only for the reports of `Blocked` verifiers in the outcome message.

### 5. `triage_findings`
This is a coordinator tool on tickets it owns: `{ticket_id, decisions: [{id, decision: fix_now | follow_up | wont_fix, reason}]}`.

- Each `id` must be `untriaged`, or `open` (see open decision 1).
- Each reason is one line, checked with the same limits as `Selection::check_reason`, through a shared helper with a neutral message.
- The call is all or nothing, records a `findings_triaged` event, and wakes nobody.
- A decided entry cannot be decided again.

`fix_now` only records the decision. The coordinator sends the fix to the implementer with `send_message`, then verifies again.
*Alternative:* have the tool route `fix_now` entries to the implementer itself. Rejected because `send_message` already does that, and the coordinator usually wants to batch the entries with context.

### 6. Caps
`VerificationSettings.max_cycles` defaults to 2 and is validated to 1–5 like `max_rounds`, whose default becomes 3. `verify_ticket` refuses when the next cycle number would exceed `max_cycles`, saying to accept with a waiver, close the ticket or ask the human. Every started cycle counts, including one an implementer's `blocked` report ended early. The human raises the cap in **Models → Review** if more cycles are warranted. That screen (`verifiers.rs`) gets a cycle-cap selector beside the round cap.

### 7. Cycle history
`Ticket.previous_cycles: Vec<Verification>`. `verify_ticket` moves the old `verification` there before creating the new cycle. `verification` stays the latest cycle, so `routing`, `running_cycle`, `cycle_verifiers` and `workspace_context` consumers are unchanged.

### 8. Verifier memory
`start_round` adds the following lines to every round's message:

- **Base and diff.** `Base: <merge-base sha> (<repository base>)` and `Diff: <base>..<commit>, <shortstat>`. From round 2 on it also adds `New since round N-1: <prev>..<commit>, <shortstat>`.
- **Already decided.** `Already decided; do not re-raise unless the cited code changed:` followed by the `wont_fix` and `follow_up` entries (id, location, summary).
- **Your open findings.** The verifier's own focus's `open` entries, with "re-report one by its id only if it is still present".

From round 2 on, the prefix becomes: "Check only whether your open blocking findings are fixed and whether the new diff introduces a blocking defect; do not raise new findings on code you already reviewed." Configured instructions are kept in every round: `next_round` passes the settings' instructions instead of `Default::default()`.

The new `worktrees.rs` helpers `merge_base(worktree, base, commit)` and `shortstat(worktree, from, to)` run through `runtime::git_output`. The base is the ticket repository's `base`, such as `origin/main`. If Git fails, the line says "base unavailable: <reason>", and verification still starts.

### 9. Patch-equivalent acceptance
`worktrees::patch_id(worktree, base, commit)` computes `git diff <merge-base> <commit>` piped to `git patch-id --stable`. It needs a small stdin-taking variant of `runtime::git_answer`. `accept_ticket` treats HEAD as the verified commit when the two SHAs are equal, or when both patch-ids exist and match. Otherwise it refuses, naming both commits, and the reason if the patch-id could not be computed. The waiver uses the same check.

### 10. `accept_ticket` gates on the latest cycle, with an optional waiver
`accept_ticket {ticket_id, waived?}`. Every acceptance requires all of the following:

- No cycle is running.
- The latest cycle's verified commit is HEAD or patch-equivalent to it.
- The worktree is clean.
- The ledger has no `untriaged` entries.

Without `waived`, the latest cycle must be `Passed`. With `waived`, the latest cycle must be `Blocked`, no verifier's latest result in it may be `Blocked` or `Pending`, and the reason must be one line. A waiver after a passed cycle is refused as having nothing to waive.

The ticket's `state` string is no longer the gate. This also fixes a live bug. Once a cycle passes, any plain implementer report, such as `completed` after opening the PR, overwrites `ticket.state` through `Routing::Plain` (`verification.rs:374`). `accept_ticket` (`live.rs:390-394`) then refuses with "Ticket is completed; independent verification must pass", even though the passed cycle's commit is HEAD. That breaks the "accept only after the PR merges" flow, and it happened on the housekeeping ticket after PR #29 merged.
*Alternative:* stop a plain report from overwriting a `passed` state. Rejected because the cycle record already holds the verdict, so gating on it removes the bug at its source instead of guarding one state among several. The ticket's `state` keeps reflecting the latest report, as today.

A waiver sets `Ticket.waiver = Some(reason)`. The cycle outcome stays `Blocked`, and `ticket_accepted`'s body becomes `<id>; waived: <reason>; open: F3, F7`. The open entries stay `open` in the ledger as the record of accepted risk.
*Alternative:* add a separate state `accepted_waived`. Rejected because `is_open`, `finish_ticket` and every consumer of "accepted" would need to learn it. A field keeps acceptance one state and the waiver one fact.

### 11. The cycle-end message
`end_round`'s outcome text lists the cycle number of `max_cycles`, the `open` entries, the `untriaged` entries, and the ids of the entries already decided. It names only actions that are legal now:

- **Passed:** "Triage the untriaged findings with triage_findings, then accept the ticket."
- **Blocked:** fix and verify again (only while cycles remain), accept with a waiver naming the open findings (only when no verifier reported blocked), close the ticket, or ask the human.

"Or start a fresh cycle" is removed. The coordinator still wakes once per cycle.

### 12. Defaults and prose
- **Default verifiers.** Tester "Tests"; Reviewer "Correctness" on Claude, with the instruction "Correctness against the ticket's acceptance criteria."; Reviewer "Regressions" on Codex, with the instruction "Regressions and test coverage." Short focuses keep agent names readable, and the instruction carries the lens.
- **`tester.md` v5.** Report every finding in `findings` with severity, `path:line`, trigger and evidence. Write no patches. The definition of blocking: it breaks an acceptance criterion, breaks existing behavior or tests, or has a concrete, reachable trigger in this change. A race blocks only with a failing test or a step-by-step `file:line` trace. Bugs that predate the change are `pre_existing`. In round 2 and later, re-check only. Respect already-decided entries.
- **`implementer.md` v4.** Replace "commit the fixes… Do not argue with or message the verifiers" with: "Fix each routed finding with the smallest change. If a finding is wrong or out of scope, tell the coordinator in one sentence with evidence instead of implementing it. Do not message the verifiers. Only make changes that are directly requested or clearly necessary. Don't add error handling, fallbacks, or validation for scenarios that can't happen."
- **`main-coordinator.md` v9.** Triage every untriaged entry once with a one-line reason. Waive with `accept_ticket`'s `waived` when only blocking findings you judge acceptable remain. Ask the human when a waiver would cover a product, security or data-loss call. The stop rule: "After two or three passes, remaining non-blocking findings become follow-up tickets or won't-fix entries. Never start a fresh cycle to chase them." Drop "call verify_ticket again for a fresh cycle with every verifier".
- **`communication.md`.** Unchanged.

### 13. Desktop
- `sidebar.rs::ticket_row`: when `state == "accepted"` and `waiver` is set, draw a dashed amber ring instead of the dot. It is a new `waived` drawing in `assets.rs::DRAWINGS`, colored `p.yellow`: a dashed ring with a check, where the check tells it apart from an agent's `disconnected` ring. Its tooltip reads "Accepted with waiver · N open findings". Every other state keeps its dot.
- `tree.rs`: `verification_lines` adds one summary line per earlier cycle. A new `ledger_lines`, or a small struct the panel renders, supplies the ledger.
- `team_view.rs::tickets`: show the waiver block, the cycle lines, and a "Decision ledger" section listing each entry's id, summary, location, source, status pill and reason. This matches `design/mockups/current/review-convergence.html`.
- `verifiers.rs`: add the cycle-cap selector.

### Files and functions to change
| File | Functions / items |
|---|---|
| `crates/workspace-core/src/lib.rs` | `Severity`, `Finding`, `ReportedFinding`, `LedgerEntry`, `EntryStatus` (new); `Ticket` (`ledger`, `previous_cycles`, `waiver`); `VerificationSettings` (`max_cycles`, `Default`, `validate`) |
| `crates/workspace-host/src/verification.rs` | `verify_ticket`, `start_round`, `report`, `record_verdict`, `end_round`, `next_round`, `findings`; new `evidenced`, `record_findings`, `triage_findings` |
| `crates/workspace-host/src/live.rs` | `accept_ticket`, `finish_ticket` (event body), `agent_tool` (`report` findings, `accept_ticket` waived, `triage_findings`) |
| `crates/workspace-host/src/mcp.rs` | `tools()`: `report.findings`, `accept_ticket.waived`, `triage_findings`; descriptions of `verify_ticket` and `accept_ticket` |
| `crates/workspace-host/src/worktrees.rs`, `runtime.rs` | `merge_base`, `shortstat`, `patch_id`; stdin-taking Git helper |
| `crates/workspace-core/src/selection.rs` | share the one-line reason check |
| `crates/workspace-host/skills/` | `tester.md` v5, `implementer.md` v4, `main-coordinator.md` v9 |
| `crates/workspace-desktop/src/` | `sidebar.rs::ticket_row`, `assets.rs`, `tree.rs::verification_lines`, `team_view.rs::tickets`, `verifiers.rs` |
| Tests | `crates/workspace-host/tests/verification.rs`, `contracts.rs` (tool list, skill versions); `tree.rs` unit tests |
| Docs | `docs/DEVELOPMENT.md` (verification and acceptance paragraphs), `docs/PRD.md` (verification paragraph and the Review card defaults) |

### Wording the open-pr skill needs (outside this repo)
Its checklist line "passed this exact head" becomes: "the ticket's latest verification cycle passed at this head or at a patch-equivalent one (same `git patch-id --stable` against the base), or the coordinator accepted it with a recorded waiver." Squashing for the PR no longer needs a new cycle.

## Risks / Trade-offs

- [Present but weak evidence still fails a round] → The host checks shape, not truth. The implementer's one-sentence objection and the coordinator's triage or waiver are the correction path.
- [A verifier omits findings and only writes prose] → Its `failed` becomes `passed` with a reason, and the prose still reaches the coordinator quietly. The tester contract and the tool schema make `findings` the obvious path.
- [Strict cycle counting] → A cycle the implementer's own blocker ended early still counts. The human raises `max_cycles`. A simple rule beats a special case.
- [A stacked ticket's diff includes its parent's commits] → The merge-base is taken against the repository base, not the parent branch. This only affects the memory lines and patch-id. A rebased stack keeps matching patch-ids when its own diff is unchanged.
- [The patch-id needs the base ref locally] → If it is missing, acceptance falls back to exact-SHA matching and says why.
- [Ledger growth in `workspace_context`] → Entries are bounded per report, and a ticket's cycles are capped, so a ticket holds at most a few dozen entries.
- [Saved settings keep old defaults] → Existing `settings.json` files keep "Claude"/"Codex" and a round cap of 2. See open decision 3.

## Migration Plan

No data migration. New `Ticket` and `VerificationSettings` fields default when absent. A ticket verified before this change has an empty ledger and no history, and its next cycle counts from its stored cycle number. Rollback: older builds ignore the new JSON fields. A ticket accepted with a waiver stays accepted under an older build.

## Open decisions for the human

1. **Overruling a routed blocking finding mid-cycle.** May `triage_findings` mark an `open` entry `wont_fix` or `follow_up`? If so, the re-check is told it is decided, and a re-report of that id does not fail the round. Recommended: yes. Without it, the implementer's sanctioned objection can only end in a waiver after the round cap.
2. **Where a waived ticket appears.** Accepting archives a ticket's agents, so the tree hides the ticket unless **Show archived** is on. The ring would mostly be seen there and in the Tickets panel. Recommended: keep today's hiding. The alternative is to keep waived tickets visible in the tree until the human dismisses them.
3. **Saved verification settings.** Recommended: no migration; the human switches to the lenses and the new round cap in **Models → Review**. The alternative is to rewrite a saved list that exactly equals the old default.
