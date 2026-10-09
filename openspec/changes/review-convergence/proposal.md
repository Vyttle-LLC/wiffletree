## Why

Verification cycles do not converge, and the host causes most of it. Any concern a verifier raises fails the round, and the host forwards every failure to the implementer under the coordinator's name, untriaged. The coordinator can only end a cycle by passing it unanimously or by starting another, uncapped one, and each new cycle starts reviewers who don't know what was already decided. A 50-line change can grow to 400 lines this way. This change moves the decision on what gets fixed back to the coordinator, records it, and guarantees every cycle ends on a decision.

## What Changes

- **Findings, not verdicts.** Verifiers attach structured findings to their `passed` or `failed` report. Each finding has a severity (`blocking`, `non_blocking` or `pre_existing`), a `file:line` location, a summary, a concrete trigger and evidence. Verifiers report every finding and write no patches.
- **The host computes the round verdict.** A verifier's result is `failed` only if it reported a blocking finding with a location, a trigger and evidence. `blocked`, meaning the verifier could not verify, and the worktree-changed override are unchanged. Only evidenced blocking findings go to the implementer, sent as Wiffletree rather than in the coordinator's name.
- **A per-ticket decision ledger.** Every finding becomes a ledger entry with an id (`F1`, `F2`, …). Routed blocking entries are `open` until their verifier's re-check clears them (`fixed`). Every other entry starts `untriaged`. A new `triage_findings` tool lets the coordinator decide each entry once: `fix_now`, `follow_up` or `wont_fix`, each with a one-line reason. The coordinator still wakes once per cycle, and that message lists the open and untriaged entries.
- **A waiver.** `accept_ticket` takes an optional `waived` reason. It is allowed only after a blocked cycle in which no verifier reported `blocked` or is still pending, whose last round's commit is HEAD or patch-equivalent to it, and whose ledger has no untriaged entries. The reason is stored on the ticket and in the `ticket_accepted` event with the open entries. The cycle's outcome stays `blocked`. The sidebar shows a distinct glyph for a waived ticket.
- **Caps.** A new `max_cycles` setting (default 2, 1–5) makes `verify_ticket` refuse once a ticket has used it. The default `max_rounds` rises from 2 to 3. The passed message no longer says "or start a fresh cycle".
- **Verifier memory.** Every round's input names the base commit, the diff range and its `--shortstat`, and the ledger's `wont_fix` and `follow_up` entries, marked as already decided. From round 2 on, a verifier re-checks only its own open blocking findings and the new diff. Configured instructions are sent in every round, not only the first.
- **Cycle history.** A new cycle no longer overwrites the previous one. Earlier cycles are kept on the ticket.
- **Patch-equivalent acceptance.** `accept_ticket` also accepts a HEAD that differs from the verified commit when its `git patch-id --stable` against the base matches. A squash or a clean rebase no longer forces a new cycle.
- **Prose and defaults.** The implementer contract replaces "Do not argue" with the avoid-over-engineering text and a one-sentence objection to the coordinator. The coordinator contract gains the triage, waiver and stop rules, and the tester/reviewer contract gains the findings format and the definition of blocking. The default reviewers get lenses (Correctness, Regressions) instead of provider names as their focus.
- **Not included.** The 2× diff-growth alarm, the PR watcher, and the open-pr skill text, which lives outside this repository. design.md records the wording the open-pr skill needs.

## Capabilities

### New Capabilities
<!-- None. The ledger and triage are part of verification. -->

### Modified Capabilities
- `ticket-verification`: findings and the host-computed verdict; routing only evidenced blocking findings, sent as the host; the decision ledger and `triage_findings`; `max_cycles` and the new round default; verifier memory and round-2 scope; cycle history; default lenses; the cycle-end message.
- `ticket-workspaces`: `accept_ticket` with a waiver and patch-equivalent HEADs, and gating on the latest cycle rather than the ticket's state string; the waived glyph on the sidebar's ticket row; the role contracts' new duties.

## Impact

- `workspace-core` (`lib.rs`): `Finding`, `Severity`, `LedgerEntry`, `EntryStatus`; new fields `Ticket.ledger`, `Ticket.previous_cycles`, `Ticket.waiver`, `VerificationSettings.max_cycles`; new defaults. Tickets are stored as JSON, so serde defaults handle old records without a schema migration.
- `workspace-host`: `verification.rs` (verdict, routing, ledger, memory, caps, history, messages), `live.rs` (`accept_ticket`, `triage_findings`, `report` arguments), `mcp.rs` (schemas), `worktrees.rs` (merge-base, shortstat, patch-id helpers), skills `implementer.md`, `main-coordinator.md`, `tester.md`.
- `workspace-desktop`: `sidebar.rs` (waived glyph), `tree.rs` and `team_view.rs` (cycle history and ledger in the Tickets panel), `verifiers.rs` (cycle cap control).
- Docs: `docs/DEVELOPMENT.md` and the verification paragraphs of `docs/PRD.md`.
- Mockup: `design/mockups/current/review-convergence.html`.
- Saved verification settings keep their verifiers and round cap; only unsaved settings get the new defaults.
