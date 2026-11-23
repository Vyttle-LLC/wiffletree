## ADDED Requirements

### Requirement: A coordinator creates a branch on origin
The workspace SHALL offer the project coordinator, and no other role, a `create_branch` tool taking `repository_id`, `branch` and an optional `start`, the name of a branch on origin, defaulting to the branch the repository's base tracks. The host SHALL refuse, pushing nothing and saying why, when the project does not use the repository, the human has not allowed its owner to publish without asking, `branch` is not a valid branch name or starts with `refs/`, `branch` is protected (see protected-branches), `branch` already exists on origin, or `start` does not. Otherwise the host itself SHALL create `branch` on origin at `start`'s commit, pushing exactly that one ref to the push URL it checked, with no tags or submodules and failing if the branch appeared meanwhile, and SHALL reply with the repository, the branch, the start and the commit.

#### Scenario: A feature branch for a plan
- **WHEN** the coordinator calls `create_branch` with `repository_id` of `Vyttle-LLC/wiffletree` and `branch` `feature/agent-permissions`
- **THEN** origin has `feature/agent-permissions` at the commit `origin/main` had, and the reply names that branch and commit

#### Scenario: Then a ticket on it
- **WHEN** the coordinator then calls `create_ticket` with `base` `feature/agent-permissions`
- **THEN** the ticket is created on that base

#### Scenario: From another branch
- **WHEN** it gives `start` `release/2026.11`, which is on origin
- **THEN** the new branch is at that branch's commit

#### Scenario: Refusals
- **WHEN** it asks for `main`, a branch that already exists, `bad..name`, a `start` not on origin, or a repository whose owner the human has not allowed
- **THEN** nothing is pushed and the result says which

#### Scenario: Other roles
- **WHEN** an implementer calls `create_branch`
- **THEN** the call is refused and nothing is pushed
