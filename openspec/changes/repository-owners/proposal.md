## Why

Wiffletree's instructions tell agents to open pull requests "by following the open-pr skill", a skill Wiffletree does not ship, and they leave the question of where an agent may push to that text. The README still says Wiffletree never pushes or opens pull requests. The human has now decided the rule (plan of October 12, 2026): agents may publish a ticket without asking only in repositories whose owner the human allowed, and everywhere else each step waits for the human's approval. A rule about what leaves the machine has to be checked by the app, not by a sentence in a prompt, so the app needs to run the push and open the pull request itself.

## What Changes

- **Owners are detected and listed.** For every workspace repository, the host reads the GitHub owner and name from the `origin` remote, and asks GitHub whether it is a fork and whether it is public, with one read-only `gh api graphql` call for the whole workspace. The Repositories page lists each organization or user with its repository count, each fork on its own, and repositories with no GitHub remote.
- **The human ticks the owners allowed to have PRs opened without asking.** The block is titled "Open pull requests without asking" and says in one line that Wiffletree never approves, merges, closes or comments on a PR. Nothing is allowed until ticked. The list is saved in this machine's host settings and is never synced. A fork is its own entry, never covered by its owner's tick. A repository with no GitHub remote, and one GitHub has not yet answered for, always ask. There is no per-repository override.
- **A new agent tool, `publish_ticket`, run by the host.** It pushes the ticket's branch to `origin` and opens the ticket's PR if it has none; called again later, it pushes the new commits. The host itself runs `git push` and `gh pr create`. It refuses unless the ticket has been reviewed, no review is running, the worktree's HEAD is the reviewed commit or a patch-equivalent one, the worktree is on the ticket's own branch, and nothing on GitHub would be overwritten that the host did not push. It sends exactly one ref, the ticket's branch, to the one push URL it checked, whatever the Git configuration says, and it confirms the repository's current owner with GitHub before anything is sent. The ticket's coordinator or its implementer may call it; a verifier may not.
- **Allowed owner and a passed review: no question. Anywhere else: one approval per step.** In an allowed owner's repository, after a passed review, the steps run at once. Otherwise each step, the push and then the PR and every later push, becomes an Inbox approval that the host writes and records, naming the repository, its visibility, the branch, the commit and, for the PR, the base branch and the full title and description. Approving runs exactly what the card showed; if any of it has changed, nothing is sent and the agent must ask again. A coordinator's word is never enough.
- **A review that ended blocked always needs the human.** In every repository, allowed owners' included, publishing after a blocked review waits for an Inbox approval whose card says the review ended blocked and lists the open findings.
- **No operation approves, merges, closes or comments.** Merging stays the human's, including into a plan's feature branch: the rule that let a plan's coordinator merge there is dropped.
- **The contract stops naming the open-pr skill.** `crates/workspace-host/skills/communication.md` line 12 loses both mentions and states the rules in Wiffletree's own words: nothing leaves the machine from an agent's shell, `publish_ticket` is the way to publish, and the PR's title and description follow the repository's and the human's own PR rules. The coordinator and implementer instructions gain one sentence each on when to call it.
- **The README and development notes are corrected.** `README.md` lines 51, 81 and 87 and `docs/DEVELOPMENT.md` "Current limits" say what Wiffletree now does and what it still cannot prevent.
- **The limit is stated, not hidden.** Agents run with permission prompts bypassed, so an agent's shell can still run `git push` or `gh pr create`. This change makes the app's operation the only path the instructions permit and checks every use of it. It adds no sandbox and scopes no credentials.
- **Not included:** review levels, the PR keeper (rebases, conflict fixes, reading comments), the Skills page, the settings repository, any model agent for PRs, per-repository overrides, draft PRs, PRs into another repository such as a fork's parent, and hosts other than `github.com`.

## Capabilities

### New Capabilities
- `repository-owners`: detecting each repository's GitHub owner, fork status and visibility; the owners list on the Repositories page; the per-machine allowed-owners setting; which repositories publish without asking; what agents are told about each repository.
- `ticket-publishing`: the `publish_ticket` tool; the checks the host makes before anything leaves the machine; the per-step human approval where the owner is not allowed; the approval a blocked review always needs; the result messages; the instructions' publishing rules; the boundary (no approve, merge, close or comment) and the stated limit.

### Modified Capabilities
- `pr-watch`: the polling cadence gains one case. A repository is checked at once after the host pushes to or opens a ticket's PR there, so the ticket row shows the PR within seconds instead of up to five minutes later.

## Impact

- `workspace-core` (`lib.rs`, new `owners.rs`): `Repository.github`, `HostSettings.allowed_owners`, `Ticket.publishing` and `Ticket.pushed`, `Snapshot.allowed_owners` and `Snapshot.github_error`, `Command::SetOwnerAllowed`, the shared rule for which repositories publish without asking, the `publish:` approval and notice prefixes.
- `workspace-host`: `repositories.rs` (reading owners and GitHub facts), a new `publish.rs` (checks, push, PR creation, approvals), `service.rs` (two background events, the tool, the approval answer), `runtime.rs` (a bounded `git push`), `pr_watch.rs` (check a repository at once), `settings.rs`, `live.rs` (`workspace_context`), `mcp.rs` (the tool definition), and the three instruction files under `skills/`.
- `workspace-desktop`: `repositories_view.rs` (owners block, row label), `inspector.rs` (the approval cards), `sidebar.rs` and `tree.rs` (a waiting line on the ticket row).
- Stored data: new optional fields on JSON already stored (repositories, tickets, `settings.json`). No table and no migration.
- Runtime dependency: the same authenticated `gh` the PR watcher uses, now also for `gh pr create`. The host's `git push` uses the human's own Git credentials and runs the repository's pre-push hooks.
- Docs: `README.md`, `docs/DEVELOPMENT.md`, and a short paragraph in `docs/PRD.md`.
- Mockup: `design/mockups/current/repository-owners.html`.
