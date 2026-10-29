## Why

Only the desktop app can create a project, through `create_project` on the client socket, and agents cannot reach that socket. When the human asks a coordinator for work that is a separate outcome, the coordinator can only widen its own project or ask the human to create one by hand.

## What Changes

- **A new workspace tool, `create_project`, for coordinators only.** It takes `name` (trimmed, at most 128 characters), `brief` and optionally `repository_ids`. Every other role is refused.
- **The project starts at once**, with no Inbox approval: its folder, a coordinator from Who runs what, and a place at the top of the sidebar, as the human's own new project gets. `repository_ids` sets its repository selection as `set_project_repositories` does; leaving it out means every workspace repository.
- **The brief is the new coordinator's first message, labelled with its source.** It is a Wiffletree notice (`project-brief:` id), never the human's, and its text names the creating coordinator and project and says it is not the human's authorization. The new coordinator still plans with the human and asks before creating tickets.
- **The creator is recorded** in `Project.created_by` (project and coordinator ids) and in the `project_created` activity.
- **The creator can follow the work.** Its `workspace_context` lists `created_projects`: each project it created, with its tickets as short entries (state, branch, pull request). It is read only. The main use: during a review the human asks for a finding to be handled separately, the coordinator triages it `follow_up` and hands it to a new project, then waits on a `schedule` timer until that project's PR merges.
- **All or nothing.** Creation, the repository selection, the brief and the start happen in one store savepoint. An unsaved model setup, an unknown repository id, an empty name or brief, or an unknown argument refuses and creates nothing.
- **No idempotency key.** No existing MCP tool pattern fits cheaply (`create_ticket` deduplicates by repository and title within one project), so each call creates a new project, with the usual slug suffixing.
- **Contract text:** coordinator core v6 says when to create a project and how to treat such a brief; the PRD and the host protocol describe the tool.
- **Out of scope:** messaging between the two coordinators, a CLI or socket path for agents outside Wiffletree, and any desktop UI change.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `workspace-host`: coordinators create projects; a project brief is a Wiffletree message.

## Impact

- `crates/workspace-core/src/lib.rs`: `Project.created_by`, `ProjectCreator`, `PROJECT_BRIEF_PREFIX` among the host notice prefixes.
- `crates/workspace-host/src/lib.rs`: `Host::create_project_from`, sharing `insert_project` with `create_project`.
- `crates/workspace-host/src/live.rs`, `mcp.rs`: the tool's dispatch and definition.
- `crates/workspace-host/src/repositories.rs`: `set_project_repositories` writes in a savepoint so it nests.
- `crates/workspace-host/skills/core/coordinator.md` (v6), contract tests, `docs/PRD.md`, `docs/host-protocol.md`.
