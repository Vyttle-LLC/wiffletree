## Why

Every ticket today passes through two coordinators. The main coordinator (`project_orchestrator`) calls `create_repo_coordinator`, briefs the repository coordinator (`task_orchestrator`), and that coordinator creates tickets, assigns agents, relays tester failures and accepts work. For a single-repository, single-ticket change such as SH-1171, the repository coordinator adds a model turn to every hand-off without adding a decision: it relays the brief down and the result up. Testers and reviewers also run one after another, because each is assigned in a separate coordinator turn after the previous report.

The ticket is already the unit that owns a repository, a worktree and a branch. Making it the only level between the coordinator and its agents removes a role, a tool and a class of relay turns, and lets one call verify a ticket with every configured verifier at once.

## What Changes

- **BREAKING:** Remove the repository coordinator role. The project coordinator (today's main coordinator) creates tickets, assigns agents, accepts and closes tickets directly. The `task_orchestrator` role can no longer be created; it remains readable only on migrated, archived sessions.
- A ticket is a workspace: one repository, one worktree and branch, and every agent working on it. Tickets record their repository themselves instead of inheriting it from a coordinator session.
- Sidebar: Coordinator → ticket workspace (labelled "repository · ticket") → agents. The repository-team level disappears.
- Parallel verification: a new `verify_ticket` tool starts all configured verifiers for a ticket in one call, pinned to the ticket's current commit. Verifiers are read-only and run concurrently. The host routes failures straight back to the same implementer session, re-runs only failed verifiers when the implementer reports ready again, and caps rounds (2 by default, configurable). At the cap the ticket becomes blocked and the coordinator is told; the human is asked only if the coordinator decides it needs them.
- MCP tools:
  - Removed: `create_repo_coordinator`, `archive_team`.
  - Added: `verify_ticket`.
  - Changed: `create_ticket` (takes `repository_id`; project coordinator only), `assign_ticket`, `accept_ticket` and `close_ticket` (project coordinator only), `accept_ticket` also archives the ticket's agents, `report` (verification-round routing; kinds unchanged), `workspace_context` (tickets replace teams).
  - Model-selection arguments (`provider`, `size`, `complexity`, `profile`) and the role policy shape are unchanged.
- Role instructions: delete `skills/repository-coordinator.md`; rewrite `skills/main-coordinator.md` as the project coordinator contract; update `skills/communication.md`, `skills/implementer.md` and `skills/tester.md` (tester and reviewer).
- Migration: on first start after upgrade, each repository coordinator's tickets and agents move under its main coordinator, undelivered messages addressed to it are re-addressed, and the repository coordinator is archived with its conversation intact. Nothing is deleted.
- One-time leftover-worktree cleanup: a dry-run listing of worktrees left by tickets closed or accepted before #19, with dirty, locked, detached or unpushed worktrees protected. Nothing is removed without the human's explicit selection.
- Documentation: `openspec/config.yaml`, `AGENTS.md`, `docs/PRD.md`, `README.md`, `design/README.md` and `docs/DEVELOPMENT.md` stop describing a three-level hierarchy. These edits land with the implementation, so the docs never describe behavior the code does not have.

## Capabilities

### New Capabilities
- `ticket-workspaces`: two-level coordination where the project coordinator owns tickets directly, each ticket being one repository, worktree and branch with its agents beneath it.
- `ticket-verification`: one-call, concurrent, commit-pinned, read-only verification of a ticket, with failures routed to its implementer and a capped number of rounds.
- `coordination-migration`: moving existing projects off repository coordinators without losing tickets, worktrees, sessions or conversations.
- `leftover-worktree-cleanup`: identifying and, only with explicit approval, removing worktrees left by tickets finished before #19.

### Modified Capabilities
<!-- None. The hierarchy requirements live in the unarchived native-workspace-foundation change; this change adds capabilities and records the supersession in design.md. -->

## Impact

- `workspace-core`: `Role::TaskOrchestrator` becomes legacy (not creatable; label kept for archived sessions); `Ticket` gains `repository_id` and a `verification` record; `Command::CreateTicket` takes `repository_id` and a coordinator id that must be a project coordinator; new `LeftoverWorktrees` and `RemoveLeftoverWorktrees` commands.
- `workspace-host`: `live.rs` (`create_ticket`, `assign_ticket`, `close_ticket`, `accept_ticket`, `report`, `agent_context`; `archive_team` and `create_repo_coordinator` removed; new `verify_ticket`), `mcp.rs` tool list, `worktrees.rs` (`ticket_repository` reads the ticket; leftover listing and removal), `service.rs` (admitting a verification round's turns together), `settings.rs` (worktree path unchanged), a store migration to `user_version = 6`, and the role skills.
- `workspace-desktop`: `sidebar.rs` tree, `creation.rs` (drop "Add repository team"; "New ticket" picks a repository), `team_view.rs` and `inspector.rs` (Tickets for the coordinator instead of Teams), `conversation.rs`, `main.rs` and `usage_view.rs` references to `TaskOrchestrator`, plus a leftover-worktrees sheet in the Repositories view.
- Stored data: `sessions.parent_id` and `tickets.coordinator_id` of migrated rows are rewritten to the main coordinator; repository coordinator sessions are archived; `messages` bodies are never rewritten.
- The `task_orchestrator` row in `policies` stays stored and untouched; whether to drop it belongs to the separate Model guide project.
- Supersedes the "Enforced hierarchy" requirement of the open `native-workspace-foundation` change (task orchestrators referencing a repository); that change's tasks are complete and it should be archived with the requirement reworded to tickets.
