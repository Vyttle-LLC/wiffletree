## Why

Once a ticket's PR opens, Wiffletree knows nothing about it. Nobody sees checks, review threads or conflicts in the tree. The ticket is accepted only when someone notices the merge and tells the coordinator. Coordinator timers can't fill the gap: each fire is a turn on the coordinator's model, and polling every 5 minutes would hit the 100-turn brake in about 8 hours. The PRD already calls for a watcher that is "ordinary background software" polling GitHub without a model (`docs/PRD.md:46`, `:384-404`). This change builds its first slice.

## What Changes

- **A read-only PR watcher in the host.** A new `pr_watch` module runs after every host event, beside the open-branch overlap check. For each repository with an open ticket, it makes one `gh api graphql` call on its own thread, never on the actor thread. The call reads each ticket branch's PR. It runs about every 60 s while the repository has an open PR and every 5 min otherwise. Failures back off, and a low rate-limit budget waits for the reset. The watcher never comments, labels, pushes, rebases, approves or merges.
- **A PR snapshot on the ticket.** `Ticket.pull_request` holds the PR's number, URL, state, draft flag, base, head SHA, merge state, check rollup, unresolved review-thread count and last comment id. The host saves it, and records a `pull_request` activity event, only when it changes. Tickets are stored as JSON, so no table or migration is needed. `workspace_context` shows the snapshot to the coordinator.
- **PR status in the UI.** A ticket row with a PR gets a second, muted line, such as "#142 · checks ✓ · 1 thread · behind". The Team panel's ticket card shows the PR with a link and when it was last checked. The Repositories page row adds its open-PR count, plus when PRs were last checked or why the check failed.
- **Merged → coordinator.** When a ticket's PR merges, the host sends the ticket's coordinator one message with id `pr:{ticket}:merged:{number}`. It names the PR, the merge commit and whether the merged head is the ticket worktree's HEAD, and tells the coordinator to call `accept_ticket`. The message comes from Wiffletree, not the human. It never contains PR titles, bodies or comment text.
- **Not included:** any model agent for PRs, host-side rebases, pushes or merges, approving or dismissing reviews, and the diff-growth alarm. Waking agents on failed checks, conflicts or comments, and bundling a PR skill, are open decisions in design.md.

## Capabilities

### New Capabilities
- `pr-watch`: polling cadence and backoff; finding a ticket's PR by branch; the snapshot and change events; the merged message; PR status on ticket rows, ticket cards and repository rows; the read-only boundary.

### Modified Capabilities
<!-- None. The ticket row requirement in ticket-workspaces is unchanged; the PR line is specified in pr-watch. -->

## Impact

- `workspace-core` (`lib.rs`): `PullRequest`, `PrState`, `MergeState`, `CheckState`; `Ticket.pull_request`; `Snapshot.pull_request_checks`.
- `workspace-host`: a new `pr_watch.rs` (query, parsing, cadence); `service.rs` (`Actor` state, `Event::PullRequests`, a refresh after every event, wake-ups, the `pr:` sender label in `turn_prompt`); `lib.rs` (the in-memory check status for `snapshot`); `live.rs` (`ticket_overview` needs nothing new, since the snapshot rides on the ticket JSON).
- `workspace-desktop`: `sidebar.rs::ticket_row` (PR line and tooltip), `tree.rs` (PR line text), `team_view.rs::tickets` (PR block), `repositories_view.rs` (open-PR count and check status), `conversation.rs` (labels `pr:` messages as Wiffletree).
- Runtime dependency: an authenticated `gh` on the host's PATH. Without it, PR status is shown as unavailable and nothing else changes.
- Docs: `docs/DEVELOPMENT.md` (current limits and the watcher) and the watcher paragraph of `docs/PRD.md`.
- Mockup: `design/mockups/current/pr-watch.html`.
