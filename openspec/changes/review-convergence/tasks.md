## 1. Core types

- [x] 1.1 In `workspace-core/src/lib.rs`, add `Severity`, `Finding`, `ReportedFinding` (finding plus optional `id`), `EntryStatus` and `LedgerEntry`, and a `Finding::is_evidenced_blocking` check (`blocking`, location ending `:<line>`, non-empty trigger and evidence). Verify with unit tests for each rejection case and serde round-trips in snake_case.
- [x] 1.2 Add `Ticket.ledger`, `Ticket.previous_cycles` and `Ticket.waiver`, all `#[serde(default)]`. Verify that a ticket JSON without them still decodes.
- [x] 1.3 Add `VerificationSettings.max_cycles` (default 2, validated 1–5). Change the `max_rounds` default to 3 and the default verifiers to Tests, Correctness (Claude, with its instruction) and Regressions (Codex, with its instruction). Verify that a saved settings file without `max_cycles` loads with 2 and keeps its verifiers, and that a cycle cap of 6 is rejected.
- [x] 1.4 Move the one-line reason check in `selection.rs` into a shared helper with a neutral message, keeping model-reason errors unchanged. Verify that the existing selection tests pass.

## 2. Git helpers

- [x] 2.1 Add a stdin-taking Git helper beside `runtime::git_answer`. In `worktrees.rs`, add `merge_base`, `shortstat` and `patch_id` (diff from the merge-base piped to `git patch-id --stable`). Verify with a temp repository that a squash of two commits has the same patch id, an extra change differs, and a missing base ref returns an error naming it.

## 3. Findings, verdict and ledger (host)

- [x] 3.1 Add an optional `findings` argument to `report` (`mcp.rs` schema, `live.rs::agent_tool`, `verification.rs::report`). Validate bounds, severity and ids before anything is stored, and record findings only for `Routing::Verdict`. An `id` must name an `open`, `wont_fix` or `follow_up` entry of the verifier's focus. Verify that a malformed finding or any other id refuses the report with nothing stored, and that a late verdict's findings are ignored.
- [x] 3.2 In `record_verdict`, compute the result from the findings: blocked stays blocked, the worktree check stays first, and the result is failed only with an evidenced blocking finding that is not a re-report of a `wont_fix` or `follow_up` entry. Set `VerifierRun.reason` when the result differs from the reported kind. Verify the spec scenarios "Failure without evidence", "Pass with an evidenced blocking finding" and "Only non-blocking findings".
- [x] 3.3 After a completed check only (`passed` or `failed` at an unchanged worktree), append ledger entries (`F<n>`, source focus, cycle and round) as `open` or `untriaged`, update a re-reported `open` id in place, and mark the verifier focus's other `open` entries `fixed`. Blocked and worktree-changed results leave the ledger unchanged. Verify the "Blocking finding fixed", "Blocking finding persists", "Re-check cannot run" and "Re-check from a changed worktree" scenarios.
- [x] 3.4 At cycle end, in `end_round` and in `Routing::Ends`, turn `open` entries whose focus had no verifier in the cycle into `untriaged`. Verify the "Verifier removed from settings" scenario.
- [x] 3.5 Make `end_round` send the implementer only the round's `open` entries, with sender `None` and the existing message id, and the fix-or-object wording. Also name each verifier with the worktree-changed override and its reason. Keep `findings()` only for blocked verifiers' reports. Verify that the message lists only the blocking entry, has no coordinator sender, and that the coordinator gets no turn, plus the "Worktree changed by a verifier" scenario.

## 4. Triage, caps, memory and history (host)

- [ ] 4.1 Add the coordinator-only `triage_findings` tool (`mcp.rs`, `live.rs::agent_tool`, `verification.rs`). It is all or nothing, accepts any decision on `untriaged` entries and only `follow_up` or `wont_fix` on `open` ones, refuses decided, unknown or duplicate ids, `fix_now` on an open entry and multi-line reasons, records `findings_triaged` and messages nobody. Verify the "Triage once", "All or nothing", "Overruling a routed finding mid-cycle" and "Fix now does not overrule" scenarios, and that a worker calling it is refused.
- [ ] 4.2 In `verify_ticket`, move the previous `verification` into `previous_cycles`, and refuse once the next cycle would exceed `max_cycles`, with the waiver, close or ask guidance. Verify the "Third cycle refused" and "Second cycle" scenarios, and that a raised cap allows cycle 3.
- [ ] 4.3 In `start_round`, add the base, the diff range with its shortstat, the new-since-last-round range from round 2 on, the already-decided entries, the verifier's own open entries and the round-2 re-check prefix. Have `next_round` pass the configured instructions. Verify the message text for round 1, round 2 and a missing base.
- [ ] 4.4 Rewrite the cycle-end message in `end_round`: cycle n of the cap, open, untriaged and decided ids, and only the legal next actions. Remove "or start a fresh cycle". Verify the "All pass", "Cap reached", "Last cycle blocked" and "Verifier cannot verify" scenarios.

## 5. Acceptance (host)

- [ ] 5.1 Rewrite `live.rs::accept_ticket` to gate on the latest cycle. It refuses while a cycle is running and while the ledger has untriaged entries, and accepts a HEAD that is equal or patch-equivalent. Verify the "Report after a passed cycle" scenario, which reproduces the bug where a `completed` report after a pass made accept refuse, plus "Squashed after verification", "Commits after verification" and "Untriaged findings block acceptance".
- [ ] 5.2 Add the optional `waived` argument (`mcp.rs` schema and description, `agent_tool`). It is allowed only after a blocked cycle with no blocked or pending verifier, and stores `Ticket.waiver`. The `finish_ticket` event body names the reason and the open ids, and the outcome and entries stay unchanged. Verify the "Accept with a waiver" and "Waiver refused" scenarios.

## 6. Role contracts

- [ ] 6.1 `skills/tester.md` v5: findings format, no patches, the definition of blocking and of a race, pre-existing, re-check scope from round 2, and already-decided entries.
- [ ] 6.2 `skills/implementer.md` v4: replace "commit the fixes… Do not argue" with the smallest-fix, one-sentence objection and avoid-over-engineering text from design decision 12.
- [ ] 6.3 `skills/main-coordinator.md` v9: triage, waiver, ask-the-human cases, the stop rule and the cycle cap. Drop "call verify_ticket again for a fresh cycle with every verifier".
- [ ] 6.4 Update `tests/contracts.rs` for the tool list (`triage_findings`, new arguments) and the skill versions and phrases. Verify the "Bundled instructions", "Implementer may object" and "Verifier findings format" scenarios.

## 7. Desktop

- [ ] 7.1 Add the `waived` drawing (a dashed ring with a check) to `assets.rs::DRAWINGS`. Use it in `sidebar.rs::ticket_row` in yellow for an accepted ticket with a waiver, with the tooltip "Accepted with waiver · N open findings". Verify with a unit test of the glyph choice and a visual check against `design/mockups/current/review-convergence.html`.
- [ ] 7.2 In `tree.rs`, add earlier-cycle lines to `verification_lines` and a ledger row model. In `team_view.rs::tickets`, show the waiver block and the "Decision ledger" section. Verify with `tree.rs` unit tests and a visual check against the mockup.
- [ ] 7.3 Add the cycle-cap selector beside the round cap in `verifiers.rs`. Verify that it saves at once and survives restart.

## 8. Docs and checks

- [ ] 8.1 Update the verification, triage and acceptance paragraphs of `docs/DEVELOPMENT.md` and the verification paragraph and Review card defaults of `docs/PRD.md`.
- [ ] 8.2 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`, and record the results in the ready_for_testing report.
