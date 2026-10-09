## 1. Remove the caps

- [ ] 1.1 Remove `Project.turn_limit`, its value in `Host::create_project` and the unused `TurnLimits`. Verify `tests/repositories.rs`, whose fixture still stores `"turn_limit":4`, loads, and that `workspace_context` and the snapshot no longer report it.
- [ ] 1.2 In `service.rs`, remove `HOST_TURNS`, `HOST_WORKER_TURNS`, `worker_turns`, `TURN_BUDGET`, the 100-turn block in `schedule` and `admit_round`, and `hold_workers`. Replace the two capacity tests (`a_round_that_does_not_fit_…`) with "nine due workers start in one pass" and "a round waiting on the implementer's worktree holds no other worker". Keep `a_round_with_a_paused_verifier_…` passing.
- [ ] 1.3 In `verify_ticket`, remove both count refusals. In `tests/verification.rs`, replace the turn-limit part of `verify_ticket_refuses_unready_dirty_running_and_oversized_cycles` with a seven-verifier call that starts seven sessions.
- [ ] 1.4 Remove `models::turn_limit_note`, its test and its use in `ModelsView::review_card`. Verify `cargo build -p workspace-desktop`.

## 2. Turn-count check-ins

- [ ] 2.0 Add `checkin_child_turns` (default 25) and `checkin_human_turns` (default 100) to `HostSettings` with serde defaults and range validation (5–500, 10–1000), plus `Host::set_check_ins` and `Command::SetCheckIns`. Test that old settings load with the defaults and verification unchanged, that out-of-range values are refused and the saved ones kept, and that unknown fields survive a save.
- [ ] 2.1 Child check-in: in `start_turn`, count the session's runs since its parent's latest message and send `check-in:{run}:turns` at each multiple of `checkin_child_turns`. Test the 25th and 50th turns, no check-in at 26, a lowered threshold of 10 applying from the next turn, a reset after a parent message, and an archived parent getting none.
- [ ] 2.2 Human check-in: narrow `Actor.turns` to sessions without a parent and raise `turn-count:{run}` at each multiple of `checkin_human_turns`, superseding the previous one. Make the step-in branch clear `turn-count` items without touching live state, and reserve the prefix in `ensure_unreserved`. Test that the item survives the turn's end, the human's message clears it, the project stays live, and worker turns do not count.
- [ ] 2.3 Test that a turn-count check-in from a verifier during a running round wakes the coordinator and leaves the cycle unchanged, alongside `a_check_in_during_a_verification_round_…`.

- [ ] 2.4 Desktop selectors (held until the mockup is approved): two preset rows built like `verifiers::cap_selector` (agent 10/25/50/100/250, coordinator 50/100/250/500/1000) in the approved place, each saving at once with `SetCheckIns` and showing the host's error. Update the native-workspace delta with the approved placement. Verify with `cargo build -p workspace-desktop` and a check of the rendered window.

## 3. Prose and docs

- [ ] 3.1 Update `main-coordinator.md` to v10 and `communication.md` to v5 with the text in design.md, and pin both in `tests/contracts.rs`. Verify with `cargo test -p workspace-host --test contracts`.
- [ ] 3.2 Update `docs/DEVELOPMENT.md` (the Models step's turn-limit note, the turn-budget sentence in the Inbox step, the check-in paragraph and the bounds bullet) and `docs/PRD.md` (the Review card's turn-limit sentences, the timer paragraph's "turn limits" and the check-in paragraph), plus the `schedules.rs` module comment. Verify with `grep -rni "turn limit\|turn budget\|100 turns" docs crates` returning only intended hits.

## 4. Verify and archive

- [ ] 4.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` and `python3 scripts/test_communications.py`, all passing.
- [ ] 4.2 Run `openspec archive remove-turn-limits`, then `openspec validate --all --strict`, which must pass.
