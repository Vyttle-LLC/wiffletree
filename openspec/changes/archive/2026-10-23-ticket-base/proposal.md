## Why

Some work belongs on a long-running branch rather than the repository's base, such as `Parental-controls`. A coordinator could only start tickets from the repository's base or stack them on another ticket, so such work had to be retargeted by hand.

## What Changes

- **`create_ticket` takes `base`.** The name of a branch on `origin`. The host fetches it and refuses a branch that is not on `origin`, `base` together with `stack_on`, and a repeated title on another base. The ticket starts from `origin/<base>`, is measured against it, and opens its pull request into it.
- **Stacks follow the parent's base.** A stacked ticket records its parent's base, so once the parent merges, the child's pull request opens into that base.
- **Watching, warnings and the keeper follow the ticket's base.** The PR watcher reads `origin` for tickets on their own base, open-branch warnings compare only tickets on the same base, and the PR keeper rebases onto the PR's base.
- **A deleted base is reported.** The coordinator is told once, and nothing is rebased, moved or pushed.
- **Unchanged.** Tickets without `base`, and publishing policy, which still checks the repository's base.

## Capabilities

### Modified Capabilities
- `ticket-workspaces`: `create_ticket` takes `base`; open-branch warnings compare tickets on the same base.
- `ticket-publishing`: pull requests open into the ticket's base, and a stacked child's into its parent's base once the parent merges.
- `pr-keeper`: rebases onto the PR's own base, and reports a deleted base.
- `pr-watch`: reads `origin` for tickets on their own base.

## Impact

- `crates/workspace-core`: `Ticket::base`.
- `crates/workspace-host`: `checkout.rs` (creation and fetch), `stacks.rs` (`effective_base`, `Base::of`), `overlaps.rs` (grouping by base, `BaseDeleted`, `base_deleted`), `keeper.rs`, `pr_watch.rs`, `service.rs`, `live.rs`, `mcp.rs`.
- Docs: the coordinator skill, `docs/PRD.md` and `docs/DEVELOPMENT.md`.
