## MODIFIED Requirements

### Requirement: The Repositories page holds the triggers and says which repositories are kept
The Repositories page SHALL show, under the owner list, a block titled "Keep pull requests up to date" with a tick box for each trigger the app offers and the quiet period beside the rebase trigger. The block SHALL say that the triggers apply only in repositories that open pull requests without asking, that they are off until ticked, and that the PR keeper never merges, approves, closes or comments on a pull request. The rebase trigger's text SHALL say that it replaces the branch on GitHub. While any trigger is on, each repository row SHALL say "kept" when the repository publishes without asking and "not kept" otherwise; with every trigger off the rows SHALL say neither. `workspace_context` SHALL say for each repository the project uses whether it is kept.

#### Scenario: A kept and an unkept repository
- **WHEN** the rebase trigger is on, `Vyttle-LLC` is allowed and `celadon-theme` is not
- **THEN** the wiffletree row says "kept" and the celadon-website row says "not kept"

#### Scenario: Everything off
- **WHEN** every trigger is off
- **THEN** no repository row says "kept" or "not kept"

#### Scenario: The line about what Wiffletree never does
- **WHEN** the human opens the Repositories page
- **THEN** the block says that the PR keeper never merges, approves, closes or comments on a pull request

### Requirement: A stacked PR follows its parent's branch
A stacked ticket's PR SHALL be rebased onto its own base branch on GitHub, the parent's branch, never onto the repository's base. Whether a rebase is the same change SHALL be judged against the ticket's base as publishing judges it (see ticket-workspaces). Once the host has merged the parent and retargeted the child's PR to the parent's base (see ticket-merging), the keeper SHALL rebase the child onto that base replaying only the child's own commits, those after the parent's last pushed head, and SHALL judge the same change by measuring the child's earlier head against the parent's branch and the rebased head against the new base.

#### Scenario: The parent's branch moves
- **WHEN** a kept stacked PR's parent branch gains a commit on GitHub, and main moves too
- **THEN** the rebase is onto the parent's new tip, is the same change, and is pushed, and the pushed branch does not contain main's new commit

#### Scenario: The parent was squash-merged by the host
- **WHEN** the host squash-merged the parent into `feature/spaces` and retargeted the kept child's PR #181 to `feature/spaces`
- **THEN** the keeper rebases only the child's own commits onto `feature/spaces`, the rebase is the same change, it is pushed, and the child's review stands
