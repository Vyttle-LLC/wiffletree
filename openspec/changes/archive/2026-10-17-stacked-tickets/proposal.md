## Why

Every ticket branches from its repository's base and every pull request opens into it, so a coordinator cannot stack dependent work in one repository. Agents may not branch or push on their own, so they report that Wiffletree will not allow it.

## What Changes

- **`create_ticket` takes an optional `stack_on`**: the id of a ticket of the same project and repository that is not closed. The new ticket records it as `stacked_on`, and its branch starts at the parent's branch tip. Without it, nothing changes.
- **A stacked ticket is measured against its parent's branch**: the reviewers' base and diff, the publish size, and the patch comparison behind accepting, publishing and the PR keeper. A rebase of the whole stack, which changes the parent's commits, needs a new review of the child.
- **Its pull request opens into the parent's branch**. `publish_ticket` refuses until the host has pushed the parent's branch. Once the parent's pull request has merged, the child's opens into the repository's base instead, since the parent's branch is finished and may be deleted.
- **The PR keeper** already rebases a PR onto the PR's own base branch, so a stacked PR follows its parent's branch and is never moved onto main. Its patch comparison now uses the ticket's base, so a parent that only gained commits leaves the child's change the same.
- **Instructions**: the `create_ticket`, `publish_ticket` and `workspace_context` tool descriptions and the coordinator's fixed core (v2) say how and when to stack.
- **Desktop**: a stacked ticket's card and the worker's Ticket card show the parent's branch under its own.
- **Not included**: stacks across repositories, retargeting an open pull request after its parent merges, and any other UI.

## Capabilities

### Modified Capabilities
- `ticket-workspaces`: a ticket may stack on another ticket in its repository.
- `ticket-publishing`: a stacked ticket's pull request base and its refusal before the parent is pushed.
- `pr-keeper`: a stacked PR's patch is compared against its ticket's base.

## Impact

- `workspace-core`: `Ticket::stacked_on`.
- `workspace-host`: `stacks.rs` (`Base`, `Host::ticket_base`, `Host::stack_parent`); `checkout.rs` creation and the prepared publish and keeper inputs; `publish.rs` `check` and `commit_size`; `verification.rs` `diff_lines`; `keeper.rs` rebase `Job::base`; `live.rs`, `service.rs` and `mcp.rs` for the tool; `skills/core/coordinator.md`.
- `workspace-desktop`: `inspector::parent_branch`, used by the Ticket card and the coordinator's ticket list.
