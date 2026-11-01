## Context

`pull_request_queries` and `record_pull_request` both skipped tickets that are not open, so a snapshot froze at acceptance. Everything that reads a pass assumed its tickets were open: `Watch::finish` paces on any open PR, `note_bases` feeds the keeper, and notices and the keeper's held wakes reach the coordinator.

## Goals / Non-Goals

**Goals:**
- A finished ticket's recorded PR ends up true, in every view of it.
- Nothing about a finished ticket wakes anyone or costs the shorter polling interval.

**Non-Goals:**
- Accepting a ticket when its PR merges. The coordinator accepts.
- Showing finished tickets differently. The wording already renders a closed PR.

## Decisions

**`Pass` carries finished tickets in their own list.** `Pass::found` stays the open tickets, and `Pass::finished` holds the rest, which only `record_pull_requests` reads. Pacing and `note_bases` therefore need no change, and any later reader of `found` is safe by default. This matters beyond pacing: an ended PR's `baseRefOid` is the base at its end, so noting it could move the keeper's view of a live base backwards. A flag on `Found` would have asked each reader to remember it.

**A ticket's state at recording time decides, not the query's.** A ticket can finish between the listing and the result, so `pull_request_notices` checks the ticket it just read and sends nothing for a finished one. `record_pull_request` skips a finished ticket with no recorded open PR, so a late pass cannot reopen an ended record.

**The keeper's guards stay, with two added.** `kept` already refuses finished tickets, but two paths ran before it for every ticket in a repository: `release_held`, which would send a held wake because the keeper no longer holds it, and `due.waiting`, whose failed move tells the coordinator. A held wake is dropped once its ticket is finished, and a finished ticket's waiting rebase is not moved. `KeeperPoll::prepare` also leaves finished tickets out, so a pass runs no Git in a removed worktree.

**`follow_ticket_work` runs for a finished ticket.** Accepting or closing re-derives the issue, but that run sees the PR as it stood. A later state change is new information: it adds one `pr` event, and a closed ticket whose PR turns out merged counts again and can derive Done. An accepted ticket derives Done whatever its PR reads, so a closed PR leaves its issue alone. The call runs only when the number or state changed, and a host merge records its own state, so nothing is handled twice.

**The merged worktree read is skipped for a finished ticket.** Its result only serves the merged message, which is not sent, and the worktree is gone.

## Risks / Trade-offs

- A finished ticket with a rebase waiting keeps its `refs/wiffletree/keeper/<ticket>` ref, since the move that would delete it could wake the coordinator. It is one ref on a rare path.
- A PR left open forever stays in the query. It costs one branch in a query the repository makes anyway, or one idle pass every 5 minutes when it is alone.
- A repository with only finished tickets to read shows no check status, because the row follows open tickets.
