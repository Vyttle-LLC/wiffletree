## 1. Publishing

- [x] 1.1 `finish_ticket` withdraws a waiting publish request through `withdraw_request`, before the agents are archived; `resolve_attention` uses a savepoint. Test accepting and closing, with the coordinator and the implementer as caller.
- [x] 1.2 Build the waiting card in `workspace_core::waiting_card` and word the push note by whether the pull request step will ask. Test the note with the owner not allowed, allowed while the item waits, and allowed after a closed pull request.
- [x] 1.3 Report `gh pr create` stopped at its limit as `StepResult::Unfinished`, with a message for the pull request step. Test the message and that the push is still recorded.
- [x] 1.4 Add `Ticket::open_pull_request` and use it in `publish.rs` (twice) and `publishing.rs`.
- [x] 1.5 Match the mockup: "· not on GitHub yet" on a first push's Branch row, the commit count in the update title, and "The human approved both steps." after two approvals only. Test each, and that one approval does not say it.

## 2. Restart

- [x] 2.1 Correct the two PRD sentences about leftover provider processes.
- [x] 2.2 Test a confirmed leftover stopped through `Service::start` in `tests/leftover_processes.rs`.
- [x] 2.3 Test a turn that outlasts `QUIT_WAIT`: `Actor::stop` returns with the run unfinished and its process running, and the next start stops it. A real provider cannot outlast the wait, because the provider thread kills its group after a one-second grace, so the test uses an actor whose provider thread never reports.
- [x] 2.4 Give the stand-in process in `tests/leftover_processes.rs` a drop guard.
- [x] 2.5 Return `Leftover::Signalled` when a signalled group still runs; record the stop time and word the note for it. Test in `provider.rs` and through `recover_unfinished_turns`.
- [x] 2.6 Settle leftover-process notes on every scheduler pass. Test a disconnected session whose process exits.

## 3. Tests

- [x] 3.1 An overall deadline in `tests/no_turn_limits.rs`. It has no test of its own: it is reached only by a regression. Checked by hand that it fails the test and leaves no provider process running.
- [x] 3.2 Run the running-turn status test for a `ready_for_testing` report, which stores `done`.
- [x] 3.3 `resolve_after` in `login_env.rs`; the test waits for the shell's child before the timeout starts.

## 4. Verify and archive

- [x] 4.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` twice and `python3 scripts/test_communications.py`, all passing.
- [x] 4.2 Run `openspec archive follow-ups-2026-10`, then `openspec validate --all --strict`, which must pass.
