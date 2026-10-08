## 1. Core types and commands

- [ ] 1.1 Add `repository_id` and an optional `verification` record (cycle, max_rounds, outcome, rounds with commit and per-verifier session_id, focus, message_id, result, reason) to `Ticket` in `workspace-core`; verify that `cargo check --workspace` passes and that old ticket JSON without the fields still decodes
- [ ] 1.2 Add `repository_id` to `Command::CreateTicket`, and add `Command::LeftoverWorktrees` and `Command::RemoveLeftoverWorktrees { ticket_ids }` with their response types; verify with serde round-trip tests
- [ ] 1.3 Keep `Role::TaskOrchestrator` only for decoding and labels, and document it as legacy; verify that `Role::ALL` users (Models view, policies) are unchanged

## 2. Store migration (user_version 6)

- [ ] 2.1 Back up `workspace.sqlite3` to `workspace.sqlite3.pre-flatten` after a WAL checkpoint, skipping it when the copy exists; verify that a test store gains the copy once
- [ ] 2.2 In one `BEGIN IMMEDIATE` transaction, re-parent tickets and child sessions from each `task_orchestrator` to its parent, set `ticket.repository_id`, re-send queued or held child messages as `migrated:{id}`, list the project coordinator's queued or held instructions in the notice without re-sending them, cancel the originals, stop the coordinator's schedules, deny its open permission attention, archive it, record `team_migrated`, queue a non-waking `migration:{id}` notice, and set `user_version = 6`; verify with a fixture store containing an open, an accepted and an archived team
- [ ] 2.3 Raise the version guard to 6 and verify that a migrated store opens twice without changes and that a failure injected mid-migration leaves version 5 intact
- [ ] 2.4 Refuse `CreateSession` and `change_archived(false)` for `task_orchestrator` sessions; verify that an archived migrated coordinator's transcript still pages

## 3. Ticket ownership and tools

- [ ] 3.1 Move `create_ticket` to the project coordinator with `repository_id` and a `Project::uses` check, keyed for idempotency by (coordinator, repository, title); verify the scenarios in `specs/ticket-workspaces`
- [ ] 3.2 Make `worktrees.rs::ticket_repository` read `ticket.repository_id`; verify that the existing worktree, restore and pending-removal tests pass
- [ ] 3.3 Remove `create_repo_coordinator` and `archive_team` from `mcp.rs` and `live.rs`; make `assign_ticket`, `accept_ticket` and `close_ticket` project-coordinator-only; make `accept_ticket` check HEAD against the last verified commit and archive the ticket's agents; verify with host tests, including that model-selection arguments resolve exactly as before
- [ ] 3.4 Replace `teams` in `workspace_context` with the coordinator's tickets (repository, state, agents, verification); verify with a context test

## 4. Verification cycle

- [ ] 4.1 Add the `verification` setting (verifiers, `max_rounds` default 2, range 1–5, unique focus) to host `settings.json` with a Settings editor; verify defaults and validation errors
- [ ] 4.2 Implement `verify_ticket` with its preconditions (ownership, `ready_for_testing`, no running cycle, clean worktree, verifier count ≤ min(`turn_limit - 1`, `HOST_WORKER_TURNS`), extracting the literal `6` host-wide worker limit in `service.rs::schedule` as that named constant), the pinned commit, verifier reuse by (role, focus) and the `verify:` messages; verify each refusal and the three-verifier start
- [ ] 4.3 In `service.rs::schedule`, treat a running cycle's testers and reviewers as readers, admit a round all-or-none, and hold new worker turns host-wide while a round waits so it cannot be starved; verify with the fake provider that three verifiers' `provider_runs` satisfy `MAX(started_at) < MIN(finished_at)`
- [ ] 4.4 Route reports during a cycle: bypass the per-report `ticket.state = kind` update so state changes only at round start (`verifying`) and round end, read-only check on verdicts, batch them to the coordinator, end the round, send one failure message to the implementer, re-run only failed verifiers on the implementer's next clean `ready_for_testing`, block at the cap with one waking message, end the cycle as `blocked` with an immediate wake on any other non-progress implementer report, and record a verdict that arrives after its cycle ended without changing the ticket's state; verify every scenario in `specs/ticket-verification`, including that `accept_ticket` refuses after one of three verifiers passed
- [ ] 4.5 Add an end-to-end SH-1171-sized test (one repository, one ticket, three verifiers, one failure fixed in round 2) asserting: no `task_orchestrator` session, every implementer or verifier input sent by the project coordinator, overlapping round-1 intervals, and exactly one worktree registered for the ticket

## 5. Role contracts

- [ ] 5.1 Delete `skills/repository-coordinator.md`, rewrite `skills/main-coordinator.md` as the coordinator contract v4 (carrying over the model-profile paragraph unchanged), and update `communication.md` (v4), `implementer.md` (v2) and `tester.md` (v3); verify that no bundled skill mentions a repository coordinator, `create_repo_coordinator` or `archive_team`
- [ ] 5.2 Update tool descriptions in `mcp.rs` to say "Coordinator"; verify with the tool-list test

## 6. Desktop

- [ ] 6.1 Rebuild `sidebar.rs::tree` as coordinator → "repository · ticket" rows → agents; verify in the running app with a migrated store
- [ ] 6.2 Remove "Add repository team" from `creation.rs` and add a repository picker to New ticket; update `conversation.rs`, `main.rs`, `inspector.rs`, `team_view.rs` and `usage_view.rs` references to `TaskOrchestrator`; verify in the running app
- [ ] 6.3 Show verification rounds (commit, verifier results) in the coordinator's Tickets inspector; verify in the running app

## 7. Leftover worktree cleanup

- [ ] 7.1 Implement `LeftoverWorktrees` (identification and classification as in design decision 8, read-only); verify with fixtures for each class, and that listing changes no file or row
- [ ] 7.2 Implement `RemoveLeftoverWorktrees`: re-classify, skip changed rows, non-forced `git worktree remove`, branches kept, activity recorded; verify each spec scenario
- [ ] 7.3 Add the "Leftover worktrees…" sheet to the Repositories view (no default selection, protected rows not selectable, confirmation naming the count); verify in the running app against a copy of the real store before showing the human the dry-run list

## 8. Documentation

These edits ship in the same phase and pull request as the role removal (groups 2, 3 and 5). They are never split into a separate or later change, so the docs and the running system always describe the same hierarchy.

- [ ] 8.1 Change the hierarchy line in `openspec/config.yaml` to `Hierarchy: project coordinator -> tickets (one repository worktree each) -> agents.`
- [ ] 8.2 Change `AGENTS.md` "Keep project → task orchestrator → agents semantics" to "Keep project → ticket → agents semantics", keeping the rest of the line
- [ ] 8.3 Update `docs/PRD.md` (principles, hierarchy table and example tree, interface, reviewers paragraph, agent communication, usage scopes and delivery stages), `README.md`, `design/README.md` and `docs/DEVELOPMENT.md`, leaving the historical mockups untouched
- [ ] 8.4 Reword the "Enforced hierarchy" requirement in `native-workspace-foundation` to tickets before that change is archived
- [ ] 8.5 Run `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` and `scripts/test_communications.py`
