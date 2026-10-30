## Context

See proposal.md for why. The code and text this builds on, at main 3be59c36:

- **History of the rule.** Commit d9b54c6a (#22, October 8, 2026) let a plan's coordinator merge into the plan's feature branch from its shell, through the open-pr skill. Commit 4b29415a (#51, repository-owners, October 13) moved publishing into the host and dropped that rule as human decision 9 (`openspec/changes/archive/2026-10-13-repository-owners/design.md:50`, `proposal.md:12`). No reason against merging was recorded; merging was outside that change's scope.
- **Text that forbids merging today.** `openspec/specs/ticket-publishing/spec.md:301-306` ("The host never approves, merges, closes or comments"), `:308-321` (the instructions say only the human merges) and `:323-328` (README and notes say it never merges); `openspec/specs/pr-keeper/spec.md:207` and `:217-219` (the Repositories block's line); `openspec/specs/repository-owners/spec.md:89` and `:107-109` (the owners block's line); `crates/workspace-host/skills/core/communication.md:12` ("never … merge a pull request yourself … Only the human merges"); `docs/PRD.md:475` ("never approves, merges, closes or comments on a PR") and `:634-638` ("Managed feature integration"); `README.md:107` ("Humans own integration"). The PR keeper's own rule (`pr-keeper/spec.md:221-226`, the keeper never merges) stays true and is not changed.
- **Repositories.** `Repository` (`crates/workspace-core/src/lib.rs:384`) holds `base`, `github` and `checks`, stored as JSON on this machine. `repositories.rs:326` reads `nameWithOwner isFork visibility` for every repository in one GraphQL call, at most daily and when a repository is added.
- **Publishing.** `publish.rs::publish_ticket` (`crates/workspace-host/src/publish.rs:506`) is the model to follow: the host runs the command, checks the review, the branch and the owner, freezes the request, asks the human through an Inbox item under the host-only `publish:` prefix (`workspace-core/src/lib.rs:2223`), and checks everything again before sending. `workspace-core/src/publishing.rs:24` (`publish_ask`) decides whether a request asks. The waiver of a blocked review publishes without asking in an allowed owner's repository (`ticket-publishing/spec.md:418-427`); without a waiver it asks (`:457`).
- **Acceptance.** `live.rs::accept_ticket` (`:387`) checks the latest cycle and calls `check_verified_head` (`:439`), which takes the verified commit or a patch-equivalent one; `ensure_findings_settled` (`:1088`) is the shared check for a waiver (every verifier checked, nothing untriaged).
- **PR watch.** `pr_watch.rs:169-170` already reads each PR's base, head, merge state, merge commit and check rollup. The first pass that records a PR as merged sends the coordinator one message telling it to accept (`pr_watch.rs:555-573`, `pr-watch/spec.md:88-110`). Only open tickets are watched.
- **Stacks.** `stacks.rs::Base::of` (`:24`) gives a stacked ticket its parent's branch while the parent's PR is unmerged and the parent's base once it merged (`pull_request_base`, `:58`). `measured` (`:41`) is always the parent's branch, and `same_patch` (`:49`) measures both commits against it. The keeper rebases with a plain `git rebase <base commit>` (`keeper.rs:595-612`). The coordinator core tells it to stack tickets and publish the parent first (`skills/core/coordinator.md:8`), and says acceptance "does not merge or publish" (`:14`).
- **Tools.** `mcp.rs::tools` (`:17`) lists them; ticket tools check `owned_ticket` (`live.rs:1048`).
- **GitHub, as the brief verified.** Sage-Social's repositories have no branch protection on `main`. A merge runs with the human's own `gh` login, so GitHub would accept a merge into `main` from the host.

## Goals / Non-Goals

**Goals:** the app, not instruction text, decides whether the host may merge; a protected branch is never merged by the host whatever anyone asks or approves; the default branch and the base are protected without the human doing anything; a failure to read GitHub never widens authority; no new table, no migration, no new scheduler.

**Non-Goals:**
- Creating, deleting or renaming branches on GitHub. The human creates feature branches.
- Approving, closing, reopening, commenting on, labelling or reviewing a pull request.
- GitHub auto-merge (`gh pr merge --auto`), merge queues and `--admin` merges.
- Changing GitHub's branch protection or rulesets.
- Retargeting stacked pull requests after a merge the human made on GitHub.
- A per-repository merge method, and merging pull requests that are no ticket's.
- A sandbox or scoped credentials (see decision 12).

## Decisions

### Who decided what

**Decided by the human on October 30, 2026, before the mockup** (from the coordinator's brief):
1. Each repository has a protected-branches list on the Repositories page, taking exact names and glob patterns such as `main` and `release/*`. The repository's default branch on GitHub and its configured base are always protected and cannot be removed.
2. A host-run tool, `merge_ticket`, for the coordinator only, merges a ticket's PR only when its base is not protected, the owner is allowed, the head is the reviewed commit or a patch-equivalent one, its checks pass and it can be merged.
3. After a blocked review the merge waits for the human's approval on an Inbox card.
4. Only the human merges into protected branches, including the feature branch into `main`. The host never merges a PR whose base is protected, whoever asks.
5. Out of scope: creating feature branches on origin, approving, commenting and closing.

**Decided by the human on October 30, 2026, approving the mockup** (`design/mockups/current/protected-branches.html`, commit 259521b4) with every recommendation in it:
6. The merge method follows GitHub: the first of squash, merge commit and rebase that the repository allows, pinned with `--match-head-commit`. No per-repository setting.
7. The protected list is stored per machine and is not synced through the settings repository.
8. GitHub's branch protection and rulesets also count as protected. If GitHub cannot be read, nothing is merged.
9. The host accepts the ticket in the same step as the merge.
10. A ticket stacked on the merged one is retargeted to the merged PR's base, and the PR keeper rebases it.
11. A merge requested while checks are running waits for them.
12. A merge after a blocked review always asks the human, waived or not. This is deliberately stricter than publishing (`ticket-publishing/spec.md:418`).

**Decided by the human on October 30, 2026, on this proposal:**
13. A pull request with no checks at all is merged on Wiffletree's own review alone. Every other condition still applies, and pending or failing checks still wait or refuse. In the human's words: "if they dont setup checks that is not on us. we did our reviews".

**Everything else in this document is a design decision made while writing the proposal, not the human's.** In particular: unreviewed commits waived at the cap also ask (decision 4); `merge_ticket` takes an optional `waived` and records the human's approval as the acceptance waiver when none is given (decisions 3 and 4); `behind` does not block a merge (decision 3); a waiting merge that needs no approval follows the host's own same-change pushes, and one that needs approval ends on any new head (decision 5); the old base stays protected only when it is still the default or on the list (decision 1); and the pattern syntax (decision 2).

### 1. What is protected
A branch is protected in a repository when any of these holds:
- It is the repository's default branch on GitHub.
- It is the repository's configured base, as a branch of `origin` (`origin/main` protects `main`). A base that is not a branch of `origin` protects nothing, and such a repository never publishes without asking anyway (`repository-owners/spec.md:70`).
- It matches an entry the human added to the repository's list.
- GitHub reports that branch protection or a ruleset applies to it.

Any active ruleset rule that `gh api repos/{o}/{r}/rules/branches/{branch}` returns for the branch counts, whatever its type. Picking rule types (`pull_request`, `required_status_checks`, `update`, `non_fast_forward`, `deletion` and so on) would leave a gap whenever GitHub adds a type; counting every rule is simpler and fails closed. A ruleset in evaluate mode is not returned by that endpoint and does not count. A ruleset that targets every branch (`~ALL`) protects every branch, so its GitHub row reads "all branches" and the block says Wiffletree can merge nothing in that repository while it applies.

Changing the base does not keep the old base protected; it stays protected only if it is also the default branch or on the list. The page shows the base row with "always protected while it is the base" so this is not a surprise.

The always-protected rows are derived each time, not stored, so they cannot be removed and cannot go stale. The human's entries are stored on the `Repository` record as `protected_branches: Vec<String>`, beside `base` and `checks`, so they stay on this machine with the rest of the record and the settings repository never sees them.

### 2. Patterns
An entry is an exact branch name or a glob, matched against the whole branch name on `origin`, without `refs/heads/`, case-sensitively:
- `*` matches any run of characters except `/`; `**` matches any run including `/`; `?` matches one character except `/`; `[...]` is a character class, with `!` or `^` negating it.
- These are the meanings GitHub gives branch protection patterns, so a pattern copied from GitHub means the same here.

The host refuses an entry, with the reason, when it is empty or has whitespace; when a class is never closed; when it starts with `refs/`; when its first segment is the name of one of the repository's remotes (`origin/main`: "Name a branch, not a remote branch"); when an exact name is not a valid branch name (`git check-ref-format --branch`); and when it is already on the list or already always protected. Matching is a small function in `workspace-core`, tested with GitHub's own examples; no glob crate is needed.

### 3. The checks before a merge
`merge_ticket` takes a ticket id and an optional one-line `waived` reason. Only the project coordinator that owns the ticket may call it (`owned_ticket` plus a role check). In order, the host refuses with the reason, and merges nothing, unless:
1. The ticket is open, has a pull request the watcher recorded as open, and no merge request for it is already waiting.
2. Acceptance would be allowed: the same checks `accept_ticket` makes (`check_verified_head`, `ensure_findings_settled`, no cycle running, no unreviewed head, the worktree clean and not locked), with `waived` standing in for the acceptance waiver. After a blocked cycle without `waived`, the reason the merge would record (decision 4) stands in, so the request can reach the human's approval; every other acceptance check (every verifier checked, nothing untriaged, the verified head) still applies. They run through the same code as `accept_ticket`.
3. The repository publishes without asking (its owner is allowed and GitHub has answered for it).
4. The host reads the PR again from GitHub (`gh pr view --json baseRefName,headRefOid,mergeable,mergeStateStatus,isDraft,state,statusCheckRollup`), and its base is the ticket's base as `stacks::Base::pull_request_base` gives it.
5. The base is not protected (decision 1). GitHub's part is read now, never from a cache: `gh api repos/{o}/{r}` (`default_branch` and the allowed merge methods), `gh api repos/{o}/{r}/branches/{base}` (`protected`) and `gh api repos/{o}/{r}/rules/branches/{base}` (rulesets that apply). All three are read-only and work with read access. If any of them fails, the merge is refused: "GitHub could not be read, so nothing was merged."
6. The PR's head is the commit the host last pushed for the ticket and the worktree's HEAD, and it is the reviewed commit or a patch-equivalent one, as `publish_ticket` checks.
7. The PR is not a draft, `mergeable` is `MERGEABLE`, and the merge state is not `DIRTY` (conflict) or `BLOCKED`. GitHub computes mergeability lazily, so `mergeable` `UNKNOWN` (or merge state `UNKNOWN`) makes the request wait like pending checks (decision 5) instead of refusing it. `BEHIND` is allowed: the base is unprotected, and GitHub merges a branch that is behind when nothing conflicts.
8. The checks rollup is `success`, or the PR has no checks at all, when Wiffletree's own review stands alone (human decision 13). `pending` makes the request wait (decision 5); `failure` or `error` refuse it.

No Inbox answer overrides checks 1 to 8. The approval in decision 4 is asked only after they hold.

### 4. When the human is asked
A merge runs without asking only when the latest verification cycle passed at the PR's head (or a patch-equivalent one) and the request carries no waiver. It asks the human, on one Inbox card under a new host-only `merge:` prefix, when:
- the latest cycle ended blocked, whether or not the coordinator passed `waived` (human decision 12); or
- the head stands on a waiver of commits no review checked, at the full-review cap.

This is stricter than `publish_ticket`, on purpose: a merge into a shared feature branch is harder to take back than a push to a ticket's own branch.

The card names the repository and whether it is public, the branches and that the base is not protected, the method and head, the review with its verifiers and how it ended, the waiver's reason if any, and the open findings, as the publish approval for a blocked review lists them. Approving covers that head only. When the human approves, the host makes every check in decision 3 again; if the head moved, the base became protected, or the checks failed, nothing is merged and the coordinator is told. If the checks are still pending, the request waits for them without asking again.

The acceptance after the merge records a waiver: the coordinator's `waived` reason when given, otherwise "Merged after a blocked review with the human's approval". The coordinator cannot pass `waived` after a passed cycle unless it covers commits, as for `accept_ticket`.

### 5. Waiting for checks
A request whose checks are pending is stored on the ticket (`Ticket.merging`: the head, the method, whether the human approved, and the cycle it was decided on). The host decides it again after each PR watcher pass for that repository, which already runs every few minutes and at once after a push, so no new scheduler is added. The request:
- merges when every check in decision 3 holds, `mergeable` is known to be `MERGEABLE` and the rollup is `success` or there are no checks;
- ends, telling the coordinator, when the checks fail, the PR closes or merges otherwise, a conflict appears, the base becomes protected, or a new verification cycle starts;
- when it needs no approval, follows the head when the host itself pushed a new head with the same change, such as the PR keeper's rebase, and waits for that head's checks; any other new head ends it;
- when it needs the human's approval (decision 4), ends on any new head, before or after the human answered, withdrawing its Inbox item, because an approval covers one head. The coordinator is told to call `merge_ticket` again, which asks again for the new head.

The ticket's PR line says "merges when they pass" while it waits. Accepting or closing the ticket withdraws it. `merge_ticket` called again while one waits is refused, naming it.

### 6. Running the merge
The host runs `gh pr merge <number> --<method> --match-head-commit <head>` in the repository, with a time limit, and never `--auto`, `--admin` or `--delete-branch`. The method is the first of squash, merge commit and rebase that `gh api repos/{o}/{r}` reports allowed. The commit's title and message are GitHub's defaults for that method. `--match-head-commit` makes GitHub refuse if the head moved between the last check and the merge.

After GitHub answers, the host reads the PR once more and records it as merged with its merge commit and `merged_by_host`, without waiting for the watcher. Then it accepts the ticket through `accept_ticket`'s code. If acceptance fails after the merge, for example because a file appeared in the worktree, the ticket stays open, the coordinator is told that the PR merged but the ticket was not accepted and why, and the ticket is then handled as any merged ticket is today.

When `gh pr merge` stops at its time limit or the host stops while it runs, the next start reads the PR: merged at the recorded head finishes as above; still open ends the request with nothing more sent, and the coordinator is told to call `merge_ticket` again.

### 7. Stacked tickets
After the host merges a ticket, for each open ticket stacked on it whose PR is open into the merged ticket's branch, the host runs `gh pr edit <child> --base <the merged PR's base>`. If GitHub already retargeted it, because the repository deletes merged branches, nothing is sent. The child's own `base` is already the parent's (`ticket-workspaces`), so the record changes nothing but its PR.

The child's branch still holds the parent's original commits, which a squash or rebase merge replaced. A plain rebase onto the new base would replay them and conflict, so:
- The keeper rebases a retargeted child with `git rebase --onto <new base commit> <parent's last pushed head>`, replaying only the child's own commits.
- `stacks::Base::same_patch` measures each commit against the ref it was built on: a commit that contains the parent's last head against the parent's branch, any other against the new base. The child's own changes then have the same patch id before and after the rebase, so its review stands.

Where the repository is not kept, or the rebase trigger is off, the coordinator's merge result says the child needs a rebase onto the new base. A rebase that conflicts or changes the patch is handed to the coordinator as any keeper rebase is. An unpublished child needs nothing: its PR already opens into the parent's base once the parent merged (`ticket-publishing/spec.md:399-416`).

### 8. What the coordinator gets back
The tool answers at once with one of: merged (with the method, head, merge commit, that the ticket was accepted, and each stacked child retargeted and whether it will be rebased); waiting for checks; waiting for the human; or refused with the reason. A waiting request's outcome arrives later as a message from Wiffletree. Messages carry the ticket, numbers, branches and short SHAs, never PR text.

### 9. What the page shows
Each repository row on the Repositories page gains a Protected branches block, collapsed to one line until opened: the lock rows, the GitHub rows, the human's entries with their current matches on `origin` and a remove button, a field to add one with the refusal shown under it, and one line naming the method the next merge would use. In a repository whose owner is not allowed the block says Wiffletree merges nothing there. The GitHub rows and the default branch come from the daily owners read, which gains `defaultBranchRef` and the repository's rulesets; the merge itself never relies on that read (decision 3). Only the human's client changes the list; there is no agent tool for it.

### 10. Instructions
The communication contract stops saying "Only the human merges". It says agents never merge from their shell; that only the coordinator merges, with `merge_ticket`, and only into a branch that is not protected; and that only the human merges into a protected branch. The coordinator core says when to call `merge_ticket` (a ticket's PR into a feature branch, once its review passed and its PR is published), that the merge accepts the ticket, that a blocked review always goes to the human, and that it must never try to merge into a protected branch or a PR into `main`. "Acceptance … does not merge or publish it" stays true and stays.

### 11. Agents see the rule
`workspace_context` shows, for each repository the project uses, its protected branches (the derived and the human's) and whether `merge_ticket` can run there, and for each ticket its waiting merge request.

### 12. What this closes and what it does not
GitHub does not protect `main` in the Sage-Social repositories, and an agent's shell runs with the human's credentials, so a shell can still run `gh pr merge`. This change makes the host's merge the only one the instructions permit and checks every use of it. It adds no sandbox and scopes no credentials.

## Risks / Trade-offs

- **The host merges with the human's credentials.** A bug in the protected check would merge into a shared branch. Mitigation: the check is a pure function over derived and stored entries, tested on its own; GitHub's protection is read at the moment of the merge; `--match-head-commit` pins the head; every check runs again after an approval and just before the merge.
- **A pattern that is too narrow.** The human can protect `release/2026.10` and forget `release/2026.11`. Mitigation: the page shows each pattern's current matches, and the default branch and base need no entry.
- **Retargeting is a write to an open PR other than its title and description.** It is limited to the base of a ticket's own stacked PR, after a merge the host made, to the branch the merged PR went into.
- **Waiting requests depend on the watcher.** If the watcher cannot read GitHub, a waiting merge waits. It never merges on stale data, because decision 3 reads GitHub again.

## Migration Plan

Repositories load with no human entries; the default branch and base are protected from the first start. No ticket has a waiting merge. Rolling back leaves an unused optional field.

## Open Questions

- **Retargeting after a merge the human made.** Not included. If the human wants it later, the PR watcher's merged message could retarget the same way.
