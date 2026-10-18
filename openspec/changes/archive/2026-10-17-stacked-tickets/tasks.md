## 1. Core

- [x] 1.1 `Ticket::stacked_on`, serde default.

## 2. Host

- [x] 2.1 `stacks.rs`: `Base`, `Host::ticket_base`, `Host::stack_parent`.
- [x] 2.2 `create_ticket` and `create_stacked_ticket` take `stack_on`; the worktree starts at the parent's branch.
- [x] 2.3 Reviewers' diff line, `commit_size`, `verified_head` and the keeper's patch comparison use the ticket's base.
- [x] 2.4 `publish::check` opens into the parent's branch, refuses before it is pushed, and uses the repository's base after the parent's PR merged.
- [x] 2.5 Tool descriptions and the coordinator core (v2).
- [x] 2.6 Tests: creation at the parent's tip and in `workspace_context`, refused parents, the publish base and refusal, the merged parent, the reviewers' diff, and the keeper following the parent's branch.

## 3. Desktop

- [x] 3.1 The parent's branch under the ticket's branch on its card and the Ticket card.

## 4. Docs

- [x] 4.1 `docs/PRD.md`.
