## Why

A project in the Vyttle Space could not create tickets in wiffletree, which Vyttle lists. Its stored list was `Some([])`: a Space's list is copied once, at New project, the two lists then drift apart, and the refusal gave no way forward. Elsewhere 24 of 26 projects use every repository in the workspace, so the selection was rarely a real choice.

On October 31, 2026 Michael approved one rule, and all six screens of `ui/mockups.html`: a project uses its Space's repositories, always current, plus any the human adds to that project. Its open tickets keep working wherever they already are.

## What Changes

- **A project uses a repository for one of three reasons.** Its Space lists it and it is in this Mac's workspace, matched by GitHub identity and worked out when it is read, so a Space edit, including one synced from another Mac, takes effect with no write to projects. The human added it to this project. Or an open ticket of this project works there, whether or not the project is archived.
- **Narrowing and "every repository" go away.** `Project.repositories: Option<Vec<String>>` becomes `Project.added_repositories: Vec<String>`. **BREAKING**: `set_project_repositories` becomes `set_added_repositories`, which takes a plain list and is the human's only.
- **Coordinators add nothing they could not before, and only at creation.** `create_project`'s `repository_ids` and an issue's listed repositories become added ones when the Space does not list them; those it lists add nothing.
- **The store upgrades once.** A project's added list is the repositories it named plus those of its open tickets, minus those its Space lists on this Mac. Archived projects are covered, and no Space's list changes.
- **Overview says where each repository comes from.** "From <Space>", "Added to this project" and, only when it applies, "An open ticket works here", naming the ticket. Add to this project replaces Choose repositories…. New project shows the Space's repositories as plain rows. The Move dialog lists gains, losses and what it keeps, and moving is never refused.
- **The refusal names the way forward**: it names the project's Space and says the human can add the repository to that Space or to this project.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `spaces`: a project inherits its Space's repositories instead of being offered them once; the Move dialog.
- `workspace-host`: `create_project`'s `repository_ids` become added repositories.
- `shared-issues`: Start project from issue uses the Space's repositories plus the issue's.
- `ticket-workspaces`: the refusal of a repository the project does not use names the way forward.
- `brain-registry`: the scenario that spoke of a project "covering every repository".

## Impact

- `workspace-core`: `Project`, `Command::SetAddedRepositories`, `Space::lists`, `Snapshot::project_repositories` and the snapshot's `open_ticket_repositories`.
- `workspace-host`: store schema 10, `repositories.rs`, `checkout.rs`, `lib.rs`, `spaces.rs`, `issue_work.rs`, `live.rs`, `mcp.rs`, remote access rows, and the coordinator's core instructions, which go to v13.
- `workspace-desktop`: `inspector.rs`, `creation.rs`, `new_project.rs`, `repositories_view.rs` and `project_name.rs`.
- Protocol: the host still accepts the app one release behind. That app reads a snapshot with no `repositories` on a project as "every repository", which is display only because the host decides, and its `set_project_repositories` is refused with a sentence.
- Docs: `docs/PRD.md`, `docs/DEVELOPMENT.md`, `docs/host-protocol.md`.
