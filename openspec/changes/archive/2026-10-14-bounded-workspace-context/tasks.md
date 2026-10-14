## 1. The reply

- [x] 1.1 In `live.rs`, make `ticket_overview` return open tickets in full and accepted or closed tickets as one short entry. Verify "A finished ticket is one line" and "An aged project stays small".
- [x] 1.2 Make archived agents short entries in `team` and leave archived children out of `child_reports`. Verify "Archived agents are short".
- [x] 1.3 Give `workspace_context` an optional `ticket_id` that returns one ticket of the caller's project in full, and say so in the tool description in `mcp.rs`. Verify "A finished ticket by id" and "A ticket of another project".

## 2. Checks

- [x] 2.1 Run fmt, clippy, the workspace tests twice, `scripts/test_communications.py` and `openspec validate --all --strict`.
