## 1. Remove the caps

- [x] 1.1 Remove `Project.turn_limit`, its value in `Host::create_project` and the unused `TurnLimits`. Verify `tests/repositories.rs`, whose fixture still stores `"turn_limit":4`, loads, and that `workspace_context` and the snapshot no longer report it.
- [x] 1.2 In `service.rs`, remove `HOST_TURNS`, `HOST_WORKER_TURNS`, `worker_turns`, `TURN_BUDGET`, the 100-turn block in `schedule` and `admit_round`, and `hold_workers`. Replace the two capacity tests (`a_round_that_does_not_fit_…`) with `a_round_waits_whole_while_the_implementer_writes_in_its_worktree`, keep `a_round_with_a_paused_verifier_waits_whole`, and add `tests/no_turn_limits.rs`, where ten workers in two projects run at once.
- [x] 1.3 In `verify_ticket`, remove both count refusals. In `tests/verification.rs`, replace the turn-limit part of `verify_ticket_refuses_unready_dirty_running_and_oversized_cycles` with a seven-verifier call that starts seven sessions.
- [x] 1.4 Remove `models::turn_limit_note`, its test and its use in `ModelsView::review_card`. Verify `cargo build -p workspace-desktop`.

## 2. Turn-count check-ins

- [x] 2.0 Add `checkin_child_turns` (default 25) and `checkin_human_turns` (default 100) to `HostSettings` with serde defaults and range validation (5–500, 10–1000), plus `Host::set_check_ins` and `Command::SetCheckIns`. Test that old settings load with the defaults and verification unchanged, that out-of-range values are refused and the saved ones kept, and that unknown fields survive a save.
- [x] 2.1 Child check-in: in `start_turn`, count the session's runs since its parent's latest message and send `check-in:{run}:turns` at each multiple of `checkin_child_turns`. Test the 25th and 50th turns, no check-in at 26, a lowered threshold of 10 applying from the next turn, and a reset after a parent message.
- [x] 2.2 Human check-in: narrow `Actor.turns` to sessions without a parent and raise `turn-count:{run}` at each multiple of `checkin_human_turns`, superseding the previous one. Make the step-in branch clear `turn-count` items without touching live state, and reserve the prefix in `ensure_unreserved`. Test that the item survives the turn's end, the human's message clears it, the project stays live, and worker turns do not count.
- [x] 2.3 Test that a turn-count check-in from a verifier during a running round wakes the coordinator and leaves the cycle unchanged, alongside `a_check_in_during_a_verification_round_…`.
- [x] 2.4 Desktop: a "Check-ins" section in the Settings window with two preset rows built like `verifiers::cap_selector` (agent 10/25/50/100/250, coordinator 50/100/250/500/1000), each saving at once with `SetCheckIns` and showing the host's error, and a shared `ui::setting` helper giving every such control a plain-words label and caption, including the Review card's "Rounds per review" and "Reviews per ticket". Update the native-workspace delta and the docs. Verify with `cargo clippy -p workspace-desktop` and a capture of the rendered Review card.

## 3. Prose and docs

- [x] 3.1 Update `main-coordinator.md` to v10 and `communication.md` to v5 with the text in design.md, and pin both in `tests/contracts.rs`. Verify with `cargo test -p workspace-host --test contracts`.
- [x] 3.2 Update `docs/DEVELOPMENT.md` (the Models step's turn-limit note, the turn-budget sentence in the Inbox step, the check-in paragraph and the bounds bullet) and `docs/PRD.md` (the Review card's turn-limit sentences, the timer paragraph's "turn limits" and the check-in paragraph), plus the `schedules.rs` module comment. Verify with `grep -rni "turn limit\|turn budget\|100 turns" docs crates` returning only intended hits.

## 4. Verify and archive

- [ ] 4.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` and `python3 scripts/test_communications.py`, all passing.
- [ ] 4.2 Run `openspec archive remove-turn-limits`, then `openspec validate --all --strict`, which must pass.
