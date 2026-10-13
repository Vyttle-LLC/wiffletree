## Why

Reviews of two merged changes, publishing tickets from the app (PR #51) and stopping provider processes a restart left running (PR #46), left small findings that were triaged as follow-ups instead of holding the merges. Three test gaps from earlier reviews wait in the same list. They are fixed together here as small changes.

## What Changes

**Publishing**

- **A finished ticket has no waiting request.** Accepting or closing a ticket withdraws its waiting publish request: the Inbox item closes, the sidebar line goes, and the caller gets the request's one message saying it was withdrawn and that nothing was pushed.
- **A waiting push card says what follows.** Once the repository's owner has been allowed while a push item waits, the card no longer promises "Opening the pull request is a separate approval." It says the pull request is then opened without asking again. A closed or merged earlier pull request still makes it a separate approval.
- **An unfinished `gh pr create` is not "no pull request".** When `gh pr create` is stopped at its 30-second limit, the caller is told the branch was pushed and the pull request may or may not have been opened, as a push stopped at its limit already says.
- **One open-pull-request rule.** `Ticket::open_pull_request` replaces three copies of the same predicate. No behavior change.
- **The mockup's wording.** A first push's Branch row reads "<branch> · not on GitHub yet". A push to an open pull request is titled "Push 1 new commit to <owner/name>?", plural when more. The success message starts "The human approved both steps." when the human approved the push and the pull request each on its own item, and not otherwise.

**Restart**

- **The PRD says what startup does.** `docs/PRD.md` said the host only probes a surviving provider process and never signals it. Startup stops the ones it can confirm; only a worktree removal just probes.
- **A stopped provider with surviving group members is reported as stopped.** When a confirmed leftover's leader exits on SIGTERM and another member of its group keeps running, the run records when the process was stopped, the agent's retry note says so, and the Inbox note says the group was signalled and part of it still runs, instead of "did not stop it".
- **A stale leftover note is settled.** The host settles a leftover-process note once it sees the group gone, whether or not that session has a turn due.
- **Tests.** A confirmed leftover stopped through `Service::start`, a turn that outlasts the 3-second quit wait and is handled by the next start, and a drop guard on the integration test's stand-in process.

**Tests**

- `tests/no_turn_limits.rs` gets an overall deadline, so a regression fails instead of hanging.
- The running-turn status test covers the `done` branch as well as `blocked`.
- `login_env::tests::timeout_kills_processes_started_by_the_shell` no longer races its 200 ms timeout against the shell starting its child.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `ticket-publishing`: a waiting request is withdrawn when its ticket is accepted or closed; the push card's Branch row, title and note; the success message after both approvals; a pull request step stopped at its time limit.
- `workspace-host`: startup reports a signalled leftover whose group members survive and records its stop time; a leftover-process note is settled when the group is gone, with no turn due.

## Impact

- `crates/workspace-core/src/lib.rs`: `Ticket::open_pull_request`, `PublishRequest.approvals`.
- `crates/workspace-core/src/publishing.rs`: `waiting_card`, the Branch row, the update title and the push note.
- `crates/workspace-desktop/src/inspector.rs`: draws the waiting card through `waiting_card`.
- `crates/workspace-host/src/publish.rs`: `withdraw_request`, `StepResult::Unfinished` for the pull request step, the success message.
- `crates/workspace-host/src/live.rs`: `finish_ticket` withdraws a waiting request.
- `crates/workspace-host/src/lib.rs`: `resolve_attention` uses a savepoint so it can run inside a ticket's close or accept.
- `crates/workspace-host/src/provider.rs`: `Leftover::Signalled`; `process_start` is public for the integration test.
- `crates/workspace-host/src/service.rs`: the signalled note and stop time, `settle_leftover_notes`.
- `crates/workspace-desktop/src/login_env.rs`: `resolve_after`, a hook between starting the shell and starting the timeout.
- `docs/PRD.md`: the two sentences about leftover provider processes.
- Tests in `workspace-core/tests/publishing.rs`, `publish.rs`, `provider.rs`, `service.rs`, `tests/leftover_processes.rs`, `tests/no_turn_limits.rs`, `login_env.rs`.
- A stored publish request gains an `approvals` count that defaults to 0. No migration.
