## 1. Watcher

- [x] 1.1 `pull_request_queries` includes finished tickets whose recorded PR is open; `TicketBranch::finished` marks them.
- [x] 1.2 `read` puts them in `Pass::finished` and skips their merged worktree read.
- [x] 1.3 `record_pull_requests` records both lists; `record_pull_request` refuses a finished ticket with no recorded open PR; `pull_request_notices` calls for nothing on a finished ticket.

## 2. Keeper

- [x] 2.1 `release_held` drops a finished ticket's held wake; `keeper_due_checked` does not move its waiting rebase; `KeeperPoll::prepare` leaves it out.

## 3. Tests

- [x] 3.1 An accepted or closed ticket's open PR read as closed and as merged, with no message, a moved archive revision, the new state in the archive and `workspace_context`, and then out of the query.
- [x] 3.2 A late pass cannot reopen an ended record; an archived coordinator is not queried; idle pacing and no noted base; a gone worktree; a ticket on a branch of origin.
- [x] 3.3 The keeper does nothing and a held wake is dropped; a linked issue notes the end once, and a merge after closing derives Done.

## 4. Docs

- [x] 4.1 `pr_watch.rs` module comment, `issue_work::derive` comment, `docs/DEVELOPMENT.md`.
