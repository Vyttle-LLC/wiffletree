## Why

When a verification cycle ends blocked, the coordinator judges whether the open findings are acceptable and may accept the ticket with a waiver. Publishing it still always raised an Inbox approval, even in an allowed owner's repository, and so did a waiver of unreviewed commits at the full-review cap. The human wants the coordinator's call to stand: the coordinator owns that judgment, and the human's real gate is merging the pull request.

## What Changes

- **`publish_ticket` takes the coordinator's waiver of a blocked review.** After a blocked cycle, `waived` is accepted under the same conditions as `accept_ticket`'s waiver: no cycle running, every verifier of the latest cycle checked, and no finding untriaged. One shared check serves both tools. At the full-review cap the waiver may still also cover commits no review checked, named by short SHA, as before.
- **Only the coordinator waives.** The implementer may still publish, but `waived` from it is refused.
- **In an allowed owner's repository a waived publish runs without asking**, both for a blocked review's open findings and for unreviewed commits at the cap. The tool's result says it is publishing under the coordinator's waiver.
- **Unchanged:** a blocked review published without a waiver still asks, once for the commit, so the coordinator can hand a real risk to the human; repositories whose owner is not allowed still ask for every step; another pull request after an ended one still asks.
- **The waiver is recorded on the ticket** (`waiver`, `waived_commits`), so a later publish of the same review and the PR keeper's same-change rebase pushes stand on it. The next verification cycle clears it.
- **Contract text:** the communication contract (v10), the coordinator core (v4), the `publish_ticket` description, the blocked-cycle message, the PRD, `docs/DEVELOPMENT.md` and the repository-owners mockup caption say: in an allowed owner's repository the workspace publishes without asking when the review passed, or when the coordinator waived a blocked review; otherwise each step waits for the human.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `ticket-publishing`: who may waive, the waiver of a blocked review, when a waived publish asks, the keeper's publish, and the instructions' wording.
- `pr-keeper`: a kept ticket whose blocked review the coordinator waived may be rebased and pushed.

## Impact

- `crates/workspace-core/src/publishing.rs`: `publish_ask` honors the waiver; `PublishAsk::UnreviewedCommits` is gone.
- `crates/workspace-core/src/lib.rs`: `Ticket::standing_review`.
- `crates/workspace-host/src/live.rs`: `ensure_findings_settled`, shared by `accept_ticket` and `publish_ticket`.
- `crates/workspace-host/src/publish.rs`, `checkout.rs`, `keeper.rs`, `verification.rs`, `service.rs`, `mcp.rs`.
- `crates/workspace-desktop/src/team_view.rs`: the waiver block of an open ticket no longer says "at acceptance".
- Skills core text, contract tests, docs, specs and the repository-owners mockup.
