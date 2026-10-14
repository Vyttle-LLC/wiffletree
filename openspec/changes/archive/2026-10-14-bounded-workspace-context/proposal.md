## Why

`workspace_context` fails for a long-running project with "Protocol message exceeds bound": the bridge refuses any line over 256 KB, and the reply grew with every ticket and agent the project ever had. The last reply that fit was 258,770 bytes, of which every ticket in full was 174 KB, reports from 75 children (74 archived) 46 KB, and 76 agents (74 archived) 28 KB. The human approved the fix on October 14, 2026: finished tickets become one line each, and the full record stays only for open tickets.

## What Changes

- Accepted and closed tickets appear in `workspace_context` as one short entry: id, title, state, repository, branch and, if it has one, its pull request's number, state and URL. Open tickets keep today's full record.
- Archived agents appear in `team` as a short entry (id, parent_id, name, role, status, `archived`; `parent_id` because coordinators already read it from every entry), and their reports leave `child_reports`.
- `workspace_context` takes an optional `ticket_id` and then returns that one ticket of the caller's project in full, whatever its state.
- Not included: raising the 256 KB limit, paging, and any other change to the reply.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `ticket-verification`: what `workspace_context` returns for tickets, the team and child reports.

## Impact

`crates/workspace-host/src/live.rs` (`agent_context`, `ticket_overview`, `child_reports`, the `workspace_context` tool arm), the tool description in `mcp.rs`, and one test in `service.rs`.
