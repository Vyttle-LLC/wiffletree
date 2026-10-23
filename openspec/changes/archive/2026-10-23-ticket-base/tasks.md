## 1. Tickets

- [x] 1.1 Add `Ticket::base`, show it in `workspace_context`, and take `base` in `create_ticket`.
- [x] 1.2 Fetch and check the branch on `origin`, refuse `base` with `stack_on` and a repeated title on another base, and start the ticket at `origin/<base>`.
- [x] 1.3 Measure a ticket against its effective base, and record a stacked ticket's parent's base.

## 2. Publishing, watching and keeping

- [x] 2.1 Open pull requests into the ticket's base, and a stacked child's into its parent's base once the parent merges.
- [x] 2.2 Read `origin` in the PR watcher for tickets on their own base, and group open-branch warnings by base.
- [x] 2.3 Report a deleted base to the coordinator once from the open-branch check and the keeper, skipping archived coordinators.

## 3. Docs

- [x] 3.1 Update the `create_ticket` and `publish_ticket` descriptions, the coordinator skill, `docs/PRD.md` and `docs/DEVELOPMENT.md`.

## 4. Verify and archive

- [x] 4.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and the tests.
- [x] 4.2 Archive the change, then run `openspec validate --all --strict`.
