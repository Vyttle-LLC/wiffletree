## Why

"Full reviews per ticket" is a return-on-investment checkpoint for the coordinator. When a ticket reaches it, the coordinator should judge whether more review is still catching real defects or only adding edge-case code, and decide on its own. Today Wiffletree tells it that only the human can raise the limit, so coordinators ask the human to raise the cap, a question the human does not want.

## What Changes

- **The cap refusal.** `verify_ticket`'s refusal at the cycle cap no longer says the human can raise the limit. It tells the coordinator that more review rarely pays for itself from here: read unreviewed changes itself, then accept with a waiver naming them, file the rest as follow-ups or close the ticket, and ask the human only for a product, security or data-loss decision.
- **The blocked-cycle message.** Once no full review remains, "ask the human" is offered only for a product, security or data-loss decision. While a full review remains, the wording is unchanged.
- **The coordinator core (v3)** says the cap is where the coordinator judges whether more review is worth it, decides itself and never asks the human to raise the cap.
- **The Coordinating stock skill (v2)** narrows "a security decision" to an open finding that leaves a real, unresolved risk, not a ticket whose security wording or code merely changed and which the coordinator has read and judged sound. The new version merges into rewrites.
- **The Reviews card's** "Full reviews per ticket" caption says the coordinator weighs more review and decides itself.

- **A waiver of unreviewed commits at the cap.** Once a ticket has used every full review, `accept_ticket` and `publish_ticket` take a `waived` reason that may cover the commits past the verified one, when it names each by its short SHA of at least 7 characters. A missing commit is refused by its SHA. The covered commits are recorded with the waiver. Below the cap, nothing changes. A publish that relies on such a waiver always waits for the human, whose Inbox item lists the commits and the reason. Commits the PR keeper took from GitHub still need a review.

The cap stays a hard limit; counting it does not change.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `ticket-verification`: the cycle cap's refusal and the blocked-cycle actions at the cap.
- `native-workspace`: the "Full reviews per ticket" caption.
- `ticket-workspaces`: accepting with a waiver of unreviewed commits at the cap.
- `ticket-publishing`: `publish_ticket`'s `waived`, and the approval it always needs.

## Impact

- `crates/workspace-host/src/verification.rs`: the cap refusal and the blocked cycle's actions.
- `crates/workspace-host/src/live.rs` (`verified_head`, `accept_ticket`), `publish.rs`, `checkout.rs`, `service.rs`, `mcp.rs`.
- `crates/workspace-core`: `Ticket.waived_commits`, `PublishRequest.waived`, `PublishAsk::UnreviewedCommits` and the approval card.
- `crates/workspace-host/skills/core/coordinator.md`, `crates/workspace-host/skills/stock/coordinating.md`, and the tests that pin their versions.
- `crates/workspace-desktop/src/verifiers.rs`: the caption.
- Docs: `docs/PRD.md`.
