## MODIFIED Requirements

### Requirement: A diverged branch is not adopted
When the fetched head and the worktree's HEAD each have a commit the other lacks, the keeper SHALL change nothing, SHALL NOT merge or rebase the two, and SHALL tell the coordinator once for that head, naming both commits. A PR head that was once the tip of the ticket's branch in this repository, as the branch's reflog records, such as a commit the implementer pushed and then amended or rebased, is the ticket's own: the keeper SHALL NOT fetch it, adopt it or tell the coordinator it diverged, and the next publish replaces it (see ticket-publishing).

#### Scenario: Both sides moved
- **WHEN** the implementer committed `aa11bb2` locally and someone pushed `5f2e7a0` to GitHub's branch
- **THEN** the worktree and GitHub's branch are unchanged, and the coordinator gets one message naming `aa11bb2` and `5f2e7a0`

#### Scenario: The implementer pushed and then amended
- **WHEN** the implementer pushed `c3d4e5f` to its open pull request's branch and then amended it locally to `9a8b7c6`
- **THEN** the keeper fetches nothing, changes nothing and sends no message, and GitHub's branch stays at `c3d4e5f` until the ticket is published again
