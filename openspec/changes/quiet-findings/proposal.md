## Why

Every finding a verifier reports enters the ticket's decision ledger, and every one that is not an evidenced blocking finding waits as `untriaged` until the project coordinator decides it with `triage_findings`. `accept_ticket` and `publish_ticket` refuse while any entry is untriaged. So the coordinator, which runs on the most expensive model, has to decide each nit one at a time before a ticket can ship.

The live workspace database shows what that costs. Across 200 tickets from 5 to 26 October 2026, verifiers raised about 750 `non_blocking` or `pre_existing` findings. Each went through `triage_findings`, and about 70% ended as `follow_up` or `wont_fix`. Blocking findings work well: the host routes them to the implementer and re-runs only the failed verifiers. The cost is the coordinator's context and turns, spent on findings that rarely change the outcome. The coordinator's context should be kept for decisions (see "When to fold subagents into your main agent loop", claude.com/resources/articles/building-agents-when-to-fold-subagents-into-your-main-agent-loop).

## What Changes

- **Non-blocking and pre-existing findings enter the ledger as `noted`, not `untriaged`.** `noted` is a new ledger status. It does not block `accept_ticket` or `publish_ticket` and needs no coordinator decision. A finding a verifier reported as `blocking` but without the evidence to fail a check stays `untriaged`, and so does an `open` entry left without its verifier when a cycle ends. Both still block acceptance as today.
- **The coordinator can still pull any noted finding into a fix in one call.** `triage_findings` accepts a `noted` entry with any of its three decisions, `fix_now`, `follow_up` or `wont_fix`, in the same all-or-nothing batch. No new tool.
- **The cycle-outcome message lists the cycle's noted findings compactly.** One paragraph gives their count and one line each (id, location, summary), capped at 10 lines, with the rest in `workspace_context`. The message no longer asks the coordinator to triage them before it accepts.
- **A keeper review that passes with only noted findings wakes no one.** It is pushed as after a clean pass.
- **Verifiers are not asked to raise noted findings again.** Each round's review memory lists them. A verifier may report one again by its id only when it now has an evidenced blocking finding about it, and that entry then becomes `open` and fails the check as any new blocking finding does.
- **Follow-ups get a destination.** When a ticket ships as a PR, its description lists the ticket's `follow_up` entries, one line each. This is a Coordinating skill rule and needs no new host storage.
- **The Reviewing skill asks each verifier to report at most five non-blocking or pre-existing findings per round**, most consequential first. The host does not enforce the limit.
- **The ticket card shows `noted` as its own status pill.** The ledger section's "untriaged" count leaves noted entries out.
- **Unchanged:** blocking findings and how they are routed, the ledger and its ids, rounds and their cap, the full-review cap, waivers, and `triage_findings`'s schema.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `ticket-verification`: the ledger's statuses and how a finding enters it, triage of `noted` entries, the cycle-outcome message, review memory, re-reporting a ledger entry, and when a passed keeper review wakes the coordinator.
- `ticket-workspaces`: the project coordinator contract's triage duty and stop rule, now in terms of `noted` findings, and the PR follow-ups list.

## Impact

- `crates/workspace-core/src/lib.rs`: `EntryStatus::Noted`; `Ticket::open_findings` leaves it out.
- `crates/workspace-host/src/verification.rs`: `record_findings`, `check_ids`, `ledger_summary`, `start_round`'s memory, `triage_findings` and the cycle-end message and keeper wake in the round-ending code.
- `crates/workspace-host/src/mcp.rs`: the `triage_findings` and `accept_ticket` descriptions.
- `crates/workspace-host/src/live.rs`: no logic change; `ensure_findings_settled` already refuses only `untriaged` entries.
- `crates/workspace-desktop/src/team_view.rs` and `wording/tickets.rs`: the `noted` pill.
- Skills: the coordinator core (v4 → v5), Coordinating (v3 → v4) and Reviewing (v1 → v2).
- `docs/PRD.md`: the verification paragraph under Role configuration and bounded model routing.
- Tests: `crates/workspace-host/tests/verification.rs`, `tests/contracts.rs`, and the inline tests in `keeper.rs` and `publish.rs`. `design.md` lists each one by behaviour.
- Stored tickets load unchanged. Entries already `untriaged` stay `untriaged`.
- The design study in `design/mockups/current/` does not show the decision ledger, so it does not change.
