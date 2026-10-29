## Why

`coordinator-waiver-publishes` let the coordinator's waiver of a blocked review publish without asking in an allowed owner's repository, but some statements still said such a publish always needs the human, and the PR keeper could not keep a branch whose commits were waived at the full-review cap.

## What Changes

- **Waived commits survive the keeper's rebase.** A head patch-equivalent to the last commit a recorded waiver covered counts as that waived head, so the keeper rebases and pushes it as it does a passed review's head. `accept_ticket` without `waived` still refuses such commits.
- **Wording.** The publish checks, the approval's review-level rule and the blocked-cycle message now allow the coordinator's waiver; so do `docs/PRD.md`, `docs/DEVELOPMENT.md` and the review-levels and repository-owners mockups.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `ticket-publishing`: the checks before anything leaves the machine, and the approval's review-level rule.
- `ticket-verification`: the blocked-cycle message offers accepting or publishing with a waiver.

## Impact

- `crates/workspace-core/src/lib.rs` (`Ticket::standing_commit`), `crates/workspace-host/src/live.rs`, `keeper.rs`, `publish.rs` tests.
- Docs and mockups.
