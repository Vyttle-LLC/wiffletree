## Why

The PR watcher stops at a ticket's acceptance or closing, so a PR that ends outside Wiffletree afterwards never refreshes. Five MUDling tickets recorded PRs as open, and showed a red conflict on Accepted cards, while GitHub said all five were closed.

## What Changes

- **A finished ticket's open PR stays watched.** An accepted or closed ticket whose recorded PR is open, and whose coordinator is not archived, stays in the watcher's query until GitHub reports the PR merged or closed. Stale records heal on the first pass, with no migration.
- **It is only recorded.** Its snapshot is saved and a linked issue follows it as before. It wakes nobody, sends no watcher message, holds no wake, gives the keeper no work, and does not keep its repository at the 60-second interval.
- **Not included:** a PR removed from GitHub, which another ticket covers. Until then, a finished ticket whose PR GitHub no longer returns stays in the query at the idle interval.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `pr-watch`: which PRs are watched, and how a finished ticket's PR is treated.

## Impact

- `workspace-host`: `pr_watch.rs` (`pull_request_queries`, `TicketBranch::finished`, `Pass::finished`, `record_pull_request`, `pull_request_notices`); `keeper.rs` (`keeper_due_checked`, `release_held`); `checkout.rs` (`KeeperPoll::prepare`); the comment on `issue_work::derive`.
- Docs: `docs/DEVELOPMENT.md`.
