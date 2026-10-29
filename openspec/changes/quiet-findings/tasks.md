## 1. Core

- [ ] 1.1 Add `EntryStatus::Noted` (label "noted") to `crates/workspace-core/src/lib.rs`. Leave it out of `is_settled` and out of `Ticket::open_findings`. Extend `workspace-core/tests/publishing.rs:173` with a `Noted` entry that `open_findings` leaves out.

## 2. Host: the ledger

- [ ] 2.1 `record_findings` (`verification.rs:213`): enter `non_blocking` and `pre_existing` findings as `noted`, and unevidenced `blocking` ones as `untriaged`. A re-report of a `noted` entry takes the new finding and round, and becomes `open` when it is evidenced blocking. Update `only_non_blocking_findings_pass_and_await_triage` (1430), the `three_findings` fixture (1643) and `workspace_context_lists_each_tickets_ledger_and_earlier_cycles` (1973). Check that `a_failure_without_evidence_passes_and_its_finding_awaits_triage` (1386) still passes unchanged.
- [ ] 2.2 `check_ids` (`verification.rs:188`): accept a `noted` id of the verifier's own focus, and update its error text. Rework the `untriaged` case in `an_id_must_name_an_open_or_settled_entry_of_the_verifiers_own_focus` (1330, 1352–1355): F2 is now `noted`, its evidenced blocking re-report is accepted and fails the check, and F2 becomes `open`.
- [ ] 2.3 `triage_findings` (`verification.rs:758`): accept `noted` like `untriaged`. Update `a_finding_is_triaged_once_and_wakes_no_one` (1656) and `a_triage_call_applies_every_decision_or_none` (1688), and add the "Pulling noted findings into a fix" scenario.
- [ ] 2.4 Add a test that `accept_ticket` and `publish_ticket` go ahead with only `noted` entries. Rewrite `untriaged_findings_block_acceptance_until_triaged` (2073) with an unevidenced blocking finding. Move the `publish.rs` inline fixtures (3142–3143, 3688–3775) that use a `NonBlocking` `Untriaged` entry as the blocker to `Blocking` severity.

## 3. Host: messages

- [ ] 3.1 `end_round` (`verification.rs:1133`) and `ledger_summary` (`verification.rs:297`): add the "Noted findings (N), not blocking:" paragraph for this cycle's `noted` entries, capped at 10 lines with "and K more in workspace_context". List earlier `noted` ids under "Already decided or noted". After a pass with no `untriaged` entry, use the new closing line. Rewrite `a_pass_with_an_untriaged_finding_asks_for_triage_then_acceptance` (1905) for a noted finding, add a test for an untriaged finding after a pass, and add one for the cap.
- [ ] 3.2 `start_round` (`verification.rs:813`): add the "Already noted" block to review memory. Extend `every_round_names_the_diff_and_the_decided_entries` (1818).
- [ ] 3.3 Keeper wake (`verification.rs:1305`): confirm that a passed keeper review with only `noted` entries sends no cycle message. Invert `a_keeper_review_that_passes_with_a_non_blocking_finding_asks_the_coordinator_to_triage` (`keeper.rs:4536`), and add an untriaged variant.
- [ ] 3.4 `mcp.rs:96` and `mcp.rs:102`: say in the `triage_findings` and `accept_ticket` descriptions that `noted` entries need no decision and can be pulled into `fix_now`. Keep the enum assertion at `contracts.rs:81` unchanged.

## 4. Skills

- [ ] 4.1 Coordinator core v5 (`skills/core/coordinator.md:10`): non-blocking and pre-existing findings are `noted` and need no decision; triage the `untriaged` ones; pull a `noted` one into a fix with `triage_findings` `fix_now`. Update the version at `contracts.rs:254`.
- [ ] 4.2 Coordinating v4: replace "triage them as follow_up or wont_fix, never fix_now" with leaving them noted, and add the PR description's "Follow-ups" list of `follow_up` entries before the "Reviewed with Wiffletree" line. Update `coordinating_at_stock_states_the_stop_rule_and_when_to_ask_the_human` (`contracts.rs:310`, the phrase at 320).
- [ ] 4.3 Reviewing v2: add the soft cap of five non-blocking or pre-existing findings per round. Update `verifiers_report_located_evidenced_findings_without_patches` (`contracts.rs:449`, the version at 462) and add the cap phrase.

## 5. Desktop

- [ ] 5.1 `ledger_list` (`team_view.rs:358`): give `Noted` a neutral pill. Keep the header count of `untriaged` only. Update the `wording/tickets.rs` fixtures (1002) where a nit is meant.

## 6. Docs and checks

- [ ] 6.1 `docs/PRD.md`: in the verification paragraph under Role configuration and bounded model routing, say that non-blocking and pre-existing findings are noted, need no decision before accept or publish, and can be pulled into a fix.
- [ ] 6.2 Run the workspace-host `verification`, `contracts` and `skills` test targets and the `keeper` and `publish` unit tests, then the full suite once before ready_for_testing.
