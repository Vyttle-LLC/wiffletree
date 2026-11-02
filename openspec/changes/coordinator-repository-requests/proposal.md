## Why

"Mudling: Initialization" is in the Personal Space, and `Vyttle-LLC/mudling-plugins` was added to the Vyttle Space, so the project never used it. Its coordinator could not tell why, guessed a restart would help, and could only ask the human to find **Add to this project…** inside Overview's Add dropdown. Project repository lists are read fresh on every call, so nothing about how they are computed needs to change. What is missing is a way to ask.

On November 1, 2026 Michael approved `ui/mockups.html`. Approving adds the repository to that one project only: no Space's list changes, nothing moves between Spaces, and the project stays in its Space.

## What Changes

- **A new coordinator tool, `request_repository`.** It takes `repository`, an `owner/name` or an absolute folder path, and `reason`, one short line. It raises an Inbox card when the repository is in the workspace but unused, or when the folder is a repository root that is not in the workspace.
- **Nothing reaches the Inbox without cause.** A repository already in use, an `owner/name` the workspace does not have and a request still waiting raise no card, and the tool says which it was. A denied request may be asked again.
- **The card** follows the approved study: the coordinator's name, the Approval pill, the title, the reason as plain text, the repository or folder, Listed by, This project, then Deny and **Add to this project**.
- **Answering is one transaction.** The answer, the change and the coordinator's message commit together, so a crash leaves the card waiting, and the message resumes the coordinator. When approval can no longer succeed, nothing changes and the message says why.
- **Coordinators are told.** `ensure_uses` and the coordinator contract point at `request_repository` instead of asking the human to find the setting.

## Capabilities

### New Capabilities

- `repository-requests`: a coordinator asking the human for a repository its project does not use.

### Modified Capabilities

- `ticket-workspaces`: the refusal of a repository outside the project now names `request_repository`.
- `spaces`: a repository no Space lists can also reach a project through a request the human approves.

## Impact

- `workspace-core`: `REPOSITORY_REQUEST_PREFIX`, `Attention.repository_request`, `RepositoryRequest`.
- `workspace-host`: a new `repository_requests.rs`, the `request_repository` tool, `ensure_uses`, `coordinator.md` (v15).
- `workspace-desktop`: the Inbox card.
- `docs/host-protocol.md`, `docs/FEATURES.md`, `docs/PRD.md`.
- Older stores load unchanged: the new `Attention` field defaults to none and is left out when none.
