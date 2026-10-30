## 1. Core

- [x] 1.1 `ReplyUpdate` and `Message::update`; `Message::from_human`; `MessageKind::trouble`.

## 2. Host

- [x] 2.1 `updates.rs`: the `reply_update` column and `reply_baselines` table, migrated before the message-revision trigger, which watches the column.
- [x] 2.2 `ProjectFacts` and the worded differences.
- [x] 2.3 `Host::reply_update` with the human-started, trouble and news rules; the turn end writes it with the reply.
- [x] 2.4 `set_plan` takes `news`; the MCP schema documents it.
- [x] 2.5 Tests: unchanged, each fact kind, human-started, trouble, news, restart.

## 3. Desktop

- [x] 3.1 `placements` folds unchanged own replies; run heading, opened run note, "No change" pill.
- [x] 3.2 "Since last update:" line on changed replies.
- [x] 3.3 `ticket_table.rs` pinned in the coordinator's conversation, with the `collapse_ticket_table` preference.
- [x] 3.4 Tests: placements beside peer runs, the since line, the table's filtering.

## 4. Instructions and docs

- [x] 4.1 Coordinating v5 and coordinator core v8, the fixtures that track the stock versions and the contract tests; the communication contract stays at v10.
- [x] 4.2 `docs/PRD.md`: Live work, Tickets inspector, coordinator section.
- [x] 4.3 Validation checks, then archive the change.
