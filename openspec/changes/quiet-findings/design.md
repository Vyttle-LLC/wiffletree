## Context

Today, a finding that is not an evidenced blocking finding enters the ledger as `untriaged` (`record_findings`, `crates/workspace-host/src/verification.rs:213`). `ensure_findings_settled` (`crates/workspace-host/src/live.rs:1023`) refuses `accept_ticket` (`live.rs:414`) and `publish_ticket` (`crates/workspace-host/src/publish.rs:793`) while any entry is `untriaged`. The cycle-outcome message from `end_round` (`verification.rs:1133`) lists every untriaged entry and, after a pass, ends with "Triage the untriaged findings with triage_findings, then accept the ticket." (`verification.rs:1278`). A keeper review that passes wakes the coordinator when it recorded an untriaged entry (`verification.rs:1305`). The coordinator core (`crates/workspace-host/skills/core/coordinator.md:10`) tells the coordinator to triage every untriaged entry. The Coordinating skill (`skills/stock/coordinating.md`) already says that non-blocking findings end the loop and should be triaged `follow_up` or `wont_fix`. So for most nits the decision is already made by policy, yet each one still costs the coordinator a call.

`untriaged` covers three kinds of entry today:

1. a `non_blocking` or `pre_existing` finding;
2. a `blocking` finding without the location, trigger or evidence needed to fail a check;
3. an `open` entry whose verifier was not in the cycle that just ended (`untriage_orphans`, `verification.rs:264`).

Only the first kind is the nit volume this change is about. The other two are claimed or once-confirmed defects that the coordinator should still decide.

Context only: the human will separately remove the Style verifier from the Full level by a settings change. That lowers the number of nits, but this change does not depend on it and works the same with or without a Style verifier.

## Goals / Non-Goals

**Goals:**
- `non_blocking` and `pre_existing` findings need no per-finding coordinator decision before accept or publish.
- The coordinator still sees them in one compact list and can pull any of them into a fix in one call.
- The coordinator's context grows by a bounded amount per cycle, not by one line per nit.

**Non-Goals:**
- Changing blocking findings, how they fail a check, or how they are routed to the implementer.
- Changing the ledger's ids, its persistence, rounds, the round cap, the full-review cap or waivers.
- A project-level follow-up tracker (see decision c).
- Rewriting entries already stored as `untriaged`.

## Decisions for the human

Each decision gives a recommendation and its trade-off. The specs delta and tasks follow the recommendations; changing a decision changes those artifacts.

### (a) Default state: a new `noted` status, not automatic `follow_up`

**Recommendation:** add `EntryStatus::Noted`. A `non_blocking` or `pre_existing` finding enters the ledger as `noted` and stays `noted` unless the coordinator decides it. `noted` does not block accept or publish, and it is not "settled": it is not a coordinator decision.

**Why not automatic `follow_up` when the cycle ends:**
- `follow_up` means "the coordinator decided to do this later" and carries the coordinator's reason. If the host wrote it by default, the ledger could no longer tell a decision from a default, and the PR's follow-up list (decision c) would fill up with nits nobody chose to keep.
- "A decision never changes" is a spec rule. Pulling an automatic `follow_up` back into `fix_now` would either break that rule or need an exception for host-made decisions. `noted` → `fix_now` is a first decision, so the rule stands.
- Until the cycle ends, an automatic `follow_up` would still be `untriaged`, so a coordinator reading `workspace_context` mid-cycle would see the old state.

**Trade-off:** one more status in the enum, the ticket card, the review memory and the PRD, and one more state for a reader to learn. Automatic `follow_up` would need no new status and no UI change.

### (b) What the coordinator sees: one capped list in the outcome message, and the existing batch call

**Recommendation:**
- The cycle-outcome message, passed or blocked, gets a paragraph "Noted findings (N), not blocking:" with one `id · location · summary` line per `noted` entry recorded in this cycle, at most 10 lines, followed by "and K more in workspace_context" when there are more.
- `ledger_summary` (`verification.rs:297`) adds `noted` ids from earlier cycles to its "Already decided" id list, renamed "Already decided or noted", so the coordinator sees each entry in full only once.
- After a pass the message ends: "Accept the ticket. To fix a noted finding first, mark it fix_now with triage_findings, send it to the implementer and verify again." When untriaged entries remain (kinds 2 and 3 above), it keeps today's "Triage the untriaged findings with triage_findings, then accept the ticket."
- On a blocked cycle, "once no finding is untriaged" stays as it is: noted entries are not untriaged, so the waiver path does not change.
- The batch call is `triage_findings` as it is. It already takes a list of decisions and applies all or none. It accepts a `noted` entry with `fix_now`, `follow_up` or `wont_fix`, and its schema does not change.

**Trade-off:** the coordinator may accept without ever reading a noted line that mattered. The cap of 10 bounds context but can hide the 11th. Today's forced decision guarantees the coordinator looked at each one. The evidence says the decision mostly confirmed the policy (70% `follow_up`/`wont_fix`), and the other 30% are still one call away. A separate batch tool, such as `fix_noted`, would make the fix path more obvious but would add a tool that does what `triage_findings` already does.

### (c) Where follow-ups go after acceptance: listed in the PR description

**Recommendation:** when a ticket ships as a PR, its description lists the ticket's `follow_up` entries under a short "Follow-ups" heading, one line each (summary and location), before the "Reviewed with Wiffletree" line. This is a Coordinating skill rule, written by whoever publishes, built from the ledger in `workspace_context`. `noted` entries are not listed: nobody chose to keep them, and they stay on the ticket's ledger. A ticket accepted without a PR keeps its follow-ups in the ledger and in the coordinator's final summary, which already has an "open follow-ups" column.

**Options considered:**
- **Nothing** (today): `follow_up` stays a ledger status with no destination, and the coordinator's reasoning is lost once the ticket is accepted.
- **A project-level follow-up list**: survives across tickets and could feed new tickets, but needs new storage, a tool and UI. That is a larger change, worth its own proposal once follow-ups are known to be acted on.
- **The host adds the section itself** at `publish_ticket`: always consistent, but the host writes no PR prose today, and an edit would have to merge with the agent's own body.

**Trade-off:** the list relies on the publishing agent following the skill, so it can be missed or out of date. It reaches the human where they already review the work, without a new mechanism.

### (d) A per-round cap on non-blocking findings in the Reviewing skill: yes, soft, five

**Recommendation:** Reviewing v2 adds: "Report at most five non_blocking or pre_existing findings per round, the most consequential first; fold findings of the same kind into one." The host does not enforce it.

**Trade-off:** a soft cap can be ignored, and a verifier may drop a sixth nit that mattered. A host-enforced cap would have to refuse the whole report, since `check_findings` refuses all or nothing (`verification.rs:169`), which costs a verifier turn and could lose its blocking findings with it. Truncating silently would hide findings without telling anyone. The outcome message's cap in (b) already bounds the coordinator's context whatever verifiers report, so the skill cap is mainly about verifier focus and ledger noise.

## Other decisions

- **Which findings are `noted`.** Only kind 1: severity `non_blocking` or `pre_existing`. A `blocking` finding that is not evidenced stays `untriaged`. The verifier claimed a defect, and the coordinator should judge whether the missing evidence matters. Orphaned `open` entries also stay `untriaged` (`untriage_orphans` is unchanged).
- **Re-reporting a noted entry.** `check_ids` (`verification.rs:188`) accepts a `noted` id of the verifier's own focus, as it does for `open`, `follow_up` and `wont_fix`. In `record_findings`, a re-report that is an evidenced blocking finding turns the entry `open` with the new finding and round, so it fails the check (`is_failing`, `verification.rs:202`, which needs no change because `noted` is not settled) and is routed to the implementer. Any other re-report updates the finding and round, and the entry stays `noted`.
- **Review memory.** `start_round` (`verification.rs:813`) lists `noted` entries in a second block after the decided ones, under "Already noted; do not raise again. Report one by its id only if you now have an evidenced blocking finding about it". Without this, later cycles would raise the same nits again as new entries.
- **Publish approval.** `Ticket::open_findings` (`crates/workspace-core/src/lib.rs:1923`), which the blocked-review approval card lists (`crates/workspace-core/src/publishing.rs:184`), leaves out `noted`. The card stays about what blocks.
- **Keeper reviews.** A passed keeper review whose only new entries are `noted` sends no cycle message (`verification.rs:1305` checks `untriaged` only, so this follows with no logic change). Withdrawing a keeper review still removes its entries, `noted` ones included.
- **Stored data.** `EntryStatus` is serde `snake_case`, so `noted` adds a variant and every stored ticket still loads. Entries already `untriaged` are not rewritten. An open ticket from before this change still needs them triaged, which affects only a few tickets and avoids a migration.
- **Ticket card.** `ledger_list` (`team_view.rs:358`) gives `Noted` a neutral pill, the same colour as `wont_fix`, labelled "noted". The ledger header's "N untriaged" count (`team_view.rs:201`) still counts only `untriaged`. The design study in `design/mockups/current/` does not show the ledger, so it does not change.

## Tests touched

Each behaviour change and the existing tests it touches. Line numbers are on origin/main at `44d3c2fa`.

| Behaviour change | `crates/workspace-host/tests/verification.rs` | `crates/workspace-host/tests/contracts.rs` | Other |
|---|---|---|---|
| Non-blocking and pre-existing findings enter as `noted` | `only_non_blocking_findings_pass_and_await_triage` (1430) expects `noted`, and is renamed; `three_findings` (1643) fixture: F2 and F3 become `noted`; `workspace_context_lists_each_tickets_ledger_and_earlier_cycles` (1973) expects `noted` for F2 and F3 | — | `workspace-core/tests/publishing.rs:173` gains a `Noted` entry left out of `open_findings`; `workspace-desktop/src/wording/tickets.rs:1002` fixture |
| Unevidenced blocking finding stays `untriaged` | `a_failure_without_evidence_passes_and_its_finding_awaits_triage` (1386), unchanged; it now guards the line between kinds 1 and 2 | — | — |
| Orphaned open entry stays `untriaged` | `an_open_entry_whose_verifier_left_the_settings_awaits_triage_when_the_next_cycle_ends` (1577) and `an_open_finding_of_a_removed_verifier_awaits_triage_after_the_next_cycle` (2683), unchanged | — | — |
| `noted` does not block accept or publish | `untriaged_findings_block_acceptance_until_triaged` (2073) is rewritten with an unevidenced blocking finding, so it still proves `untriaged` blocks; a new test accepts with only `noted` entries | — | `publish.rs` inline tests at 3142–3143 and 3688–3775 use a `NonBlocking` `Untriaged` entry as the untriaged blocker; change its severity to `Blocking`, or the fixture to `Noted` where it means a nit |
| `triage_findings` decides a `noted` entry | `a_finding_is_triaged_once_and_wakes_no_one` (1656) and `a_triage_call_applies_every_decision_or_none` (1688) assert `noted` where they assert `Untriaged` for F2 | The `triage_findings` enum assertion (81) is unchanged | `keeper.rs` inline triage tests (4604–4633, 4694–4802) keep their `untriaged` fixture |
| Outcome message lists noted findings, capped, and does not ask to triage them | `a_pass_with_an_untriaged_finding_asks_for_triage_then_acceptance` (1905) now expects "Noted findings (1), not blocking:" and the new closing line, and is renamed; a new test covers the cap of 10 and "and K more" | — | — |
| A passed keeper review with only noted findings wakes no one | — | — | `keeper.rs` `a_keeper_review_that_passes_with_a_non_blocking_finding_asks_the_coordinator_to_triage` (4536) inverts: no cycle message, pushed; `a_keeper_review_passing_wakes_the_coordinator_only_for_its_own_untriaged_entries` (5090), unchanged |
| A noted id may be re-reported; evidenced blocking turns it `open` | `an_id_must_name_an_open_or_settled_entry_of_the_verifiers_own_focus` (1330): the `untriaged` re-report of F2 (1352–1355) is now accepted and fails the check, with F2 `open`; a `foreign` re-report is still refused | — | — |
| Review memory lists noted entries | `every_round_names_the_diff_and_the_decided_entries` (1818) gains the "Already noted" block | — | — |
| Coordinator core v5 | — | `bundled_instructions_describe_the_flat_hierarchy` (246): version string at 254 | — |
| Coordinating v4 | — | `coordinating_at_stock_states_the_stop_rule_and_when_to_ask_the_human` (310): the phrase at 320 changes, and phrases are added for the PR follow-ups list and pulling noted findings into a fix | — |
| Reviewing v2 with the soft cap | — | `verifiers_report_located_evidenced_findings_without_patches` (449): version string at 462 and the new cap phrase | — |

## Risks / Trade-offs

- **A real defect reported as `non_blocking` is never read.** → The outcome message shows it in one line. A later verifier can raise it to `open` by its id with evidence. The coordinator can still `fix_now` it in the same call.
- **More undecided entries pile up in long-lived tickets' ledgers.** → `workspace_context` already shows finished tickets as one line. Open tickets' ledgers are bounded by the per-round soft cap and the round and cycle caps.
- **Skill drift.** The core, Coordinating and Reviewing all change wording. → `contracts.rs` pins each phrase, and the skills' version bumps let rewritten skills be merged.
