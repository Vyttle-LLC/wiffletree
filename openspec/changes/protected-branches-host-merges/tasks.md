## 1. Protected branches

- [ ] 1.1 Add `Repository.protected_branches` and a `workspace-core` matcher and validator for exact names and globs (`*`, `**`, `?`, `[...]`), tested with GitHub's own pattern examples and every refusal.
- [ ] 1.2 Derive the always-protected branches (the GitHub default branch and the base's branch of `origin`), and read `defaultBranchRef` and the repository's rulesets in the daily owners read.
- [ ] 1.3 Add `Command::SetProtectedBranches`, gated by the app's client credential like `SetOwnerAllowed`; keep the entries out of the settings repository and Spaces.
- [ ] 1.4 Show protected branches and whether merging is possible in `workspace_context`.

## 2. merge_ticket

- [ ] 2.1 Add the `merge_ticket` tool, coordinator only, with an optional `waived`.
- [ ] 2.2 Share acceptance's checks with `accept_ticket`, and run every check of the "checks before a merge" in order, reading the PR, the default branch, the allowed methods, branch protection and rulesets from GitHub at the time of the merge; refuse when any read fails.
- [ ] 2.3 Record a waiting merge on the ticket; decide it again after each PR watcher pass; follow the host's own same-change pushes; end it on failed checks, a conflict, a protected base, a new cycle or another head, and on any new head when it needs the human's approval; withdraw it on accept or close.
- [ ] 2.4 Ask the human under a host-only `merge:` prefix after a blocked review, waived or not, and for commits waived at the cap; check everything again on approval and before merging.
- [ ] 2.5 Merge with `gh pr merge --<method> --match-head-commit`, the method being the first GitHub allows of squash, merge commit and rebase; never `--auto`, `--admin` or `--delete-branch`.
- [ ] 2.6 Record the PR as merged by the host and accept the ticket; tell the coordinator when acceptance fails after the merge.
- [ ] 2.7 Retarget each open stacked child's PR to the merged PR's base, skipping one GitHub already moved.
- [ ] 2.8 Finish or end a merge interrupted by a restart or a time limit.
- [ ] 2.9 Test with a fake `gh`: every refusal, a PR with no checks, a blocked review without `waived` reaching the approval, a protected base whoever asks, `SetProtectedBranches` without the credential, GitHub unreadable, waiting then merging, waiting then failing, the keeper's rebase while waiting, the blocked-review approval and denial, a moved head after approval, method choice, acceptance after the merge, a stacked child, and a restart mid-merge.

## 3. PR keeper and stacks

- [ ] 3.1 Rebase a retargeted child with `git rebase --onto <new base> <parent's last pushed head>`.
- [ ] 3.2 Measure a commit's patch against the ref it was built on in `stacks::Base::same_patch`, so the child's review stands after the rebase; test it with a squash-merged parent.

## 4. Desktop

- [ ] 4.1 Draw the Protected branches block on each repository row, as in `design/mockups/current/protected-branches.html`.
- [ ] 4.2 Draw the merge approval card in the Inbox.
- [ ] 4.3 Show "merges when they pass", "merge waits for your approval", "yours to merge" and "merged into … · by Wiffletree" on the PR line, and the host's merge on the Team card.
- [ ] 4.4 Reword the owners block's and the keeper block's lines about merging.

## 5. Text and docs

- [ ] 5.1 Reword the communication contract (bump its version) and the coordinator core to name `merge_ticket` and the protected rule; update the contract tests.
- [ ] 5.2 Update `docs/PRD.md` ("Git and PR lifecycle", "Managed feature integration"), `README.md` ("Humans own integration" and the warning), `docs/DEVELOPMENT.md` and `design/README.md`.
