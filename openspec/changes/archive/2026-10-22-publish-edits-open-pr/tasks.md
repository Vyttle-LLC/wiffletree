## 1. Publishing

- [x] 1.1 Add `PublishStep::EditPullRequest` and `PublishRequest::edits`, frozen from the open pull request when a title or description is given.
- [x] 1.2 Accept and validate title and description on an open pull request; run `gh pr edit` with only the given fields after the push, or alone when GitHub already has the commit.
- [x] 1.3 Show the new title and description on the edit's approval card, and say where the branch is when an edit fails or is denied.

## 2. Docs

- [x] 2.1 Update the `publish_ticket` tool description and `docs/DEVELOPMENT.md`.

## 3. Verify and archive

- [x] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`.
- [x] 3.2 Archive the change, then run `openspec validate --all --strict`.
