## Context

A ticket's base was always `repository.base`, read in several places: worktree creation, the reviewers' diff line, `commit_size`, `live::verified_head` (accepting, publishing, the keeper's "may change the branch"), the keeper's rebase patch comparison, and `publish::check` for `gh pr create --base`.

## Decisions

**One value for a ticket's base.** `stacks::Base` is either the repository's base or a stack on a parent, carrying the parent's branch, title, whether the host pushed it and whether its PR merged. `Host::ticket_base` builds it from the store; the off-thread publish recheck builds it from the store it already reads. Every place above takes it instead of `repository.base`, so a ticket without a parent behaves exactly as before.

**The parent's local branch is the measured ref.** Ticket worktrees share the repository's refs, so `wiffletree/<parent>` is visible to every worktree and is current even before the parent is pushed. The merge-base of the child and that branch is the fork point, so reviewers see only the child's commits.

**Patch equivalence is against the parent's branch alone.** A child rebased onto a parent that only gained commits keeps its patch. A match against the repository's base is not accepted: the aggregate diff can equal the reviewed one while the child's own changes differ, for example a commit that reverts a parent fix. So a child rebased with its whole stack, as after the keeper rebased the parent onto main, needs a new review.

**After the parent's PR merges, publish targets the repository's base.** The parent's branch is finished then and GitHub may delete it. Opening into it would put the change where nobody merges it. Retargeting a child PR that is already open is left to GitHub and the human.

**"Pushed" is the host's record.** Agents never push, so `parent.pushed` is set exactly when the parent's branch reached GitHub through Wiffletree. The check needs no network.

**The keeper.** Its rebase already fetches and rebases onto `origin/<pr.base>`, and `keeper_base` is keyed by `(repository, pr.base)`, so a parent's branch works as a key. Only its patch comparison changed, to the ticket's base, so it agrees with publishing.

**Instructions in the fixed core, not the Coordinating skill.** Stacking is a tool mechanic, which the core describes next to `create_ticket`. Changing the Coordinating stock text means a new stock version, which every rewrite of it must merge, and its tests pin the v1 text.

## Risks

- Overlap warnings compare open branches against the repository's base, so a parent and its child share files and show an overlap without conflicts.
- A parent rebased or squashed after its child branched needs a new review of the child, and its next rebase may conflict, as with any stacked PR.
