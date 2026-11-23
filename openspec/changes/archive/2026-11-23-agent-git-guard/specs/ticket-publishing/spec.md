## MODIFIED Requirements

### Requirement: The host never overwrites commits it did not push
The host SHALL record the commit it last pushed for each ticket. It SHALL push a ticket's branch only when GitHub's branch is absent, is behind the commit being pushed, or is at a commit the host may replace, as after a squash or a clean rebase. When GitHub's branch is already at the commit being published, as after the implementer pushed it, the host SHALL push nothing and go on to open or update the pull request. The host MAY replace GitHub's branch head only when it is the commit the host last pushed, or a commit that was once the tip of this ticket's branch in this repository on this machine, as the branch's reflog records, such as one the implementer pushed from its shell. Otherwise it SHALL refuse and push nothing.

#### Scenario: New branch
- **WHEN** GitHub has no branch for the ticket
- **THEN** the host creates it at the reviewed commit

#### Scenario: Rebased after the host's push
- **WHEN** GitHub's branch is at the commit the host last pushed and the worktree was cleanly rebased
- **THEN** the host replaces GitHub's branch with the rebased commit

#### Scenario: The implementer pushed the reviewed commit
- **WHEN** the implementer ran `git push origin HEAD:<its branch>` at the commit whose review passed, and the coordinator then calls `publish_ticket`
- **THEN** the host pushes nothing and opens the pull request

#### Scenario: The implementer pushed an earlier commit
- **WHEN** the implementer pushed a work-in-progress commit, then committed on top of it, and the review passed at the later commit
- **THEN** the host pushes the later commit as an ordinary push and opens or updates the pull request

#### Scenario: Pushed from a shell before the host published tickets
- **WHEN** the implementer's shell pushed the ticket's branch earlier, the host has recorded no push for the ticket, and the worktree was then amended or cleanly rebased
- **THEN** the host replaces GitHub's branch with the new commit, because GitHub's head was once this branch's tip here

#### Scenario: Someone else's commits
- **WHEN** GitHub's branch has a commit the host did not push and the worktree does not contain
- **THEN** the host refuses, pushes nothing, and says the branch has commits Wiffletree did not push

### Requirement: Instructions state the publishing rules in Wiffletree's own words
The bundled instructions for every role, the fixed core with every skill at stock (see skills), SHALL NOT name an open-pr skill or any other skill for pull requests. The human's own rules and rewrites MAY name their own PR rules or skill, and the host SHALL NOT filter them. The fixed core SHALL tell agents that Wiffletree guards their shell's `git` and `gh` and refuses the rest; that only an implementer may push, only its own ticket's branch to `origin`, and only until the ticket's pull request is open, as `git push origin HEAD:<branch>`, with `--force-with-lease` and never another force; never to create, edit, comment on, review, close or merge a pull request from their shell, and not to work around the guard; to publish with `publish_ticket` once the ticket's review has ended, which opens or updates the pull request whether or not the branch was pushed; that the workspace runs the push without asking when the human allowed the repository's owner and the review passed or the coordinator waived a blocked review, and otherwise only after the human approves each step; to write the pull request's title and description by the repository's and the human's own PR rules; that only the coordinator merges, with `merge_ticket`, and only into a branch that is not protected; that only the coordinator creates a branch on origin, with `create_branch`; and that only the human merges into a protected branch. The bundled instructions SHALL NOT say that any agent may merge from its shell. The coordinator's core SHALL say when to publish, that after a blocked cycle it publishes with `waived` under the same rules as accepting with a waiver, that publishing without `waived` hands the decision to the human, and that it creates a branch a ticket needs as its base with `create_branch`. The implementer's core SHALL tell it to write the title and description when asked to publish, and that it may push its own branch to save work until the pull request is open.

#### Scenario: No pull request skill
- **WHEN** the host builds the instructions for any role with every skill at stock
- **THEN** they do not contain "open-pr" and they name `publish_ticket`

#### Scenario: The human's own PR rules
- **WHEN** the human's rules for Implementing say "Follow my open-pr skill"
- **THEN** implementers get that rule as written, and the core's publishing rules are unchanged

#### Scenario: Merging
- **WHEN** the host builds the instructions for the project coordinator
- **THEN** the core says it merges only with `merge_ticket` into a branch that is not protected, that only the human merges into a protected branch, and that it creates branches with `create_branch`

#### Scenario: Other roles
- **WHEN** the host builds the instructions for an implementer
- **THEN** the core says it may push only its own ticket's branch, never to merge a pull request from the shell, and gives it no case in which it may merge

### Requirement: The limit is documented
The README and the development notes SHALL say that Wiffletree pushes a ticket's branch and opens its pull request only through this checked operation, without asking for owners the human allows and after the human's approval elsewhere; that it merges a ticket's pull request only through `merge_ticket`, only into a branch that is not protected and only in an allowed owner's repository; that it never merges into a protected branch and never approves, closes or comments on a pull request; that agents run with their provider's permission prompts bypassed, with the human's credentials; that Wiffletree's guard refuses an agent's pull request writes and every push but an implementer's to its own ticket's branch before its pull request is open; and that the guard is a guardrail, not a sandbox, so a full path to the real `git` or `gh`, or a call to GitHub with the human's token, is not stopped.

#### Scenario: README
- **WHEN** a reader opens the README's warning and its "Humans own integration" paragraph
- **THEN** neither says Wiffletree never pushes, opens or merges pull requests, the paragraph says the human merges into protected branches, and the warning says the guard refuses agents' pushes and pull request writes but that an agent can still run any command, including a full path to `git` or `gh`
