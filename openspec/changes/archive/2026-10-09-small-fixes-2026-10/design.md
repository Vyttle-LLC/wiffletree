## Context

Eight small fixes the human approved together. Most change only a few lines. This records the three choices that are not obvious from the code.

## Decisions

### 1. The watcher re-reads its tickets at most once a minute between passes
`refresh_pull_requests` used to read sessions, tickets and repositories after every event that changed anything. Now a change only marks the watcher's ticket list stale. The store is read when a pass is due, or when the list is stale and the last read was at least `ACTIVE_MS` (60 s) ago. While the list is stale, the host arms a wake-up for that time, so a new ticket on a quiet host is still picked up. This matches `refresh_overlaps`, which also avoids a store read on most events.

*Alternative:* re-read only on the commands that can change open tickets (create, close, accept, archive, repository changes). Rejected because that list would need maintaining as commands are added.

### 2. "Working" is a display overlay, not a stored status
A mid-turn report sets the stored status to `blocked` or `done`, and `close_ticket` and `archive_agent` depend on that: a reported agent's still-running turn only defers removing the worktree. So the stored status stays. The host keeps the ids of sessions in a turn in memory (`Host.in_turn`), filled and emptied where the actor starts and finishes turns. The snapshot and `workspace_context` show such a session as `working`. The set is empty after a restart, so a crashed turn never shows as working. `workspace_context` already lists each child's recent reports, which is where the last report kind appears.

*Alternative:* defer the reported status to the end of the turn. Rejected because it changes when `close_ticket` and `archive_agent` allow a reported agent, which the ticket-workspaces spec pins.

### 3. One scheduler row per recipient
The scheduler query returned up to 100 queued messages, coordinators first, before skipping paused, held and busy sessions. It now groups by recipient and takes each one's oldest due message, so each session appears once and no `LIMIT` is needed: the result is bounded by the number of sessions. Ordering is unchanged: coordinators first, then by their oldest message.

### 4. Host notice ids
`workspace_core::host_notice` already labels `pr:` messages. It now also returns "Wiffletree" for the other ids the host sends without a sender: `verification:`, `worktree-kept:` and `migration:`. `answer:` messages carry the human's inbox answer and stay the human's. Check-ins and timer fires already name a sender (the child, or "your timer") and are unchanged.

## Risks / Trade-offs

- A new ticket on a host whose watcher read the store less than a minute ago waits up to a minute for its first PR check, where it used to be checked at once. A new ticket has no PR for a while anyway.
- `in_turn` duplicates the actor's `active` map for display. Both change only at turn start and finish, in the same functions.
