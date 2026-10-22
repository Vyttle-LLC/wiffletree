## Why

Once a ticket's pull request is open, nobody in the workspace can change its title or description: `publish_ticket` refuses both, and agents may not run `gh pr edit` themselves. Descriptions go stale after review fixes and the human edits them by hand, although GitHub allows editing at any time.

## What Changes

- **Edit through `publish_ticket`.** With an open pull request, `publish_ticket` takes an optional title and an optional description, validated as when opening one. After the push it runs `gh pr edit` with only the given fields. When GitHub already has the commit the host pushed, the edit is the only step.
- **Same rules.** The checks, the allow-list and approvals are unchanged; an edit that asks shows the new words in full on its card. A failed edit says where GitHub's branch is.
- **Unchanged.** A call with neither field pushes only, and the PR keeper's publish stays push-only.

## Capabilities

### Modified Capabilities
- `ticket-publishing`: `publish_ticket` edits an open pull request's title and description.

## Impact

- `crates/workspace-core`: `PublishStep::EditPullRequest`, `PublishRequest::edits` and the edit's approval card.
- `crates/workspace-host/src/publish.rs`: inputs, the edit step and its results; `mcp.rs`: the tool's description.
- Docs: `docs/DEVELOPMENT.md`.
