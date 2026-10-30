## Why

A project that works on a feature branch opens one pull request per ticket into that branch, and then one into `main`. Today every one of those merges is the human's, because the repository-owners change dropped the rule that let a plan's coordinator merge into its feature branch (human decision 9 of `openspec/changes/archive/2026-10-13-repository-owners/design.md`), and the specs now forbid the host to merge at all. The coordinator already decides the order of the work, reviews each ticket and publishes it. Clicking Merge on a feature branch adds a wait to the human's day without adding a decision. The human's real gate is the merge into `main` and into other branches they name.

GitHub cannot be the guard. Sage-Social's repositories have no branch protection on `main`, and a merge runs with the human's own credentials, so GitHub accepts whatever the host sends. Like publishing, the rule has to be checked by the app, not stated in a prompt.

## What Changes

- **Each repository has a list of protected branches.** It sits in the repository's row on the Repositories page and takes exact names and glob patterns such as `main` and `release/*`. The repository's default branch on GitHub and its configured base are always protected, shown with a lock, and cannot be removed. Branches GitHub itself protects, through branch protection or a ruleset, count as protected too, shown read-only. Each pattern shows the branches on `origin` it matches now. A pattern that cannot be read, or a name with a remote in front, is refused with the reason. The list is stored on this machine and is never synced.
- **A new host-run tool, `merge_ticket`, for the project coordinator only.** The host merges the ticket's pull request with `gh pr merge --match-head-commit <reviewed head>` only when all of these hold: the pull request's base on GitHub is not protected; the repository publishes without asking (its owner is allowed); the pull request's head is the reviewed commit, or a patch-equivalent one as `publish_ticket` checks; its checks passed; and GitHub says it can be merged with no conflict. Anything else is refused at once and nothing is merged. While checks are still running, the request waits and merges when they pass.
- **The merge method follows GitHub.** The host uses the first method the repository allows, in the order squash, merge commit, rebase. There is no per-repository setting.
- **A merge after a blocked review always asks the human**, waived or not, on an Inbox card that names the branches, the method, the review, the waiver's reason and the open findings. This is deliberately stricter than publishing, where the coordinator's waiver runs without asking in an allowed owner's repository.
- **Only the human merges into a protected branch,** including the final pull request from the feature branch into `main`. The host refuses `merge_ticket` there whoever asks, and no Inbox answer overrides it.
- **The merge accepts the ticket.** In the same step the host accepts the ticket as `accept_ticket` would; the tool refuses up front when acceptance would be refused. A pull request the human merges on GitHub keeps today's flow: the coordinator is told and accepts.
- **Stacked tickets follow the merge.** When the host merges a ticket, each open ticket stacked on it whose pull request opens into the merged branch is retargeted on GitHub to the merged pull request's base, and the PR keeper rebases it onto that base where the repository is kept. A rebase with the same change keeps its review.
- **Text that forbids merging is reworded,** in `ticket-publishing`, `pr-keeper`, `repository-owners` and `settings-repository`, the communication contract and coordinator core, the PRD's "Git and PR lifecycle" and "Managed feature integration" sections, the README and the development notes. They now say that the host merges only through `merge_ticket` and never into a protected branch, and that agents never merge from their shell.
- **Not included:** creating feature branches on GitHub (the human does), deleting merged branches, approving, closing, commenting on or labelling a pull request, GitHub auto-merge, retargeting stacked pull requests after a merge the human made, and changing GitHub's own protection settings.

## Capabilities

### New Capabilities
- `protected-branches`: each repository's list of protected branches; the branches that are always protected; branches GitHub protects; pattern syntax and validation; where the list is stored; the block on the Repositories page; what agents are told.
- `ticket-merging`: the `merge_ticket` tool; who may call it; the checks before any merge; waiting for checks; the approval after a blocked review; the merge method; accepting the ticket; retargeting stacked tickets; results, restarts and the stated limit.

### Modified Capabilities
- `ticket-publishing`: the host no longer offers "no merge at all" but merges only through `merge_ticket`; the instructions' merging rule; what the README and development notes say.
- `pr-keeper`: the Repositories page block's line about merging; rebasing a stacked pull request after its parent merged.
- `pr-watch`: the PR line and Team card of a pull request the host merged.
- `repository-owners`: the owners block's line about merging.
- `settings-repository`: protected branches stay on this machine.

## Impact

- `workspace-core` (`lib.rs`): `Repository.protected_branches`, a pattern matcher and validator, `Ticket.merging` (the waiting request) and `PullRequest.merged_by_host`, `Command::SetProtectedBranches`, and a `merge:` approval prefix reserved for the host.
- `workspace-host`: a new `merge.rs` (checks, GitHub reads, `gh pr merge`, approvals, waiting for checks, acceptance and retargeting), `repositories.rs` (the default branch and protection read with the owners), `pr_watch.rs` (re-deciding a waiting merge after each pass), `keeper.rs` and `stacks.rs` (rebasing a retargeted child with `--onto`), `live.rs` (`workspace_context`, sharing acceptance checks), `mcp.rs` (the tool), `service.rs`, and the core instruction files under `skills/core/`.
- `workspace-desktop`: `repositories_view.rs` (the protected-branches block), `inspector.rs` (the merge approval card), `agents_panel.rs` and `team_view.rs` (the PR line and card after a host merge).
- Stored data: new optional fields on JSON already stored (repositories, tickets). No table and no migration.
- Runtime dependency: the same authenticated `gh`, now also for `gh pr merge`, `gh pr edit --base` and three read-only `gh api` reads at each merge.
- Docs: `README.md`, `docs/DEVELOPMENT.md`, `docs/PRD.md`, `design/README.md`.
- Mockup: `design/mockups/current/protected-branches.html`, approved by the human on October 30, 2026.
