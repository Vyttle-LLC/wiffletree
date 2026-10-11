## 1. Core types and rules (`workspace-core`)

- [ ] 1.1 In `src/steps.rs`, add `PlanCounts { done, total, current }` and `Step.plan: Option<PlanCounts>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`). Verify that a stored step without the field decodes and that a step with it round-trips.
- [ ] 1.2 Create `src/progress.rs` with `BarState`, `Stage`, `Phase`, `Word`, `AgentProgress`, `TicketProgress`, `ProjectProgress`, `Board` and `ProgressPage`, and export the module from `src/lib.rs`. Verify that the types round-trip through JSON.
- [ ] 1.3 In `src/progress.rs`, implement the agent rules of `derive` (finished, blocked, in progress, indeterminate, idle, and the figure's word). Verify with one test per scenario of the spec's "Agent progress" requirement.
- [ ] 1.4 Implement the ticket rules (the six-row table in design.md decision 4, the blocked state, the indeterminate ticket, positions in hundredths of a percent, and the crew's agent list with the latest cycle's archived verifiers). Verify with one test per scenario of "Ticket progress", including "Second verification cycle" and "Fixes after a pass", plus the crew scenarios "Archived passed verifier" and "Earlier cycle's verifiers".
- [ ] 1.5 Implement the project rules (closed tickets left out, the exact mean, the phase label, the coordinator's checklist when no ticket is counted). Verify with one test per scenario of "Project progress", including the 56% four-ticket example.
- [ ] 1.6 In `src/lib.rs`, add `Command::Progress { after: u64 }`, answered with `ProgressPage`. Verify that `cargo check --workspace` passes.

## 2. Plan counts in the stream (`workspace-host`)

- [ ] 2.1 Check which plan tool the pinned `claude -p --output-format stream-json` emits (`TodoWrite` or task tools) and record the answer in the report; if it is not `TodoWrite`, stop and ask the coordinator before mapping another tool.
- [ ] 2.2 In `src/stream.rs`, make `plan()` fill `StepUpdate`'s counts and current item (the first unfinished item, bounded by `MAX_STEP_TITLE`) for Claude `TodoWrite` and Codex `todo_list`, and carry them onto `Step` where steps are built. Verify by extending the existing fixture tests (`stream.rs:587`, `:656`) to assert the counts and the current item, including a plan with every item done (`current` is `None`) and an empty plan.

## 3. Host board and delivery (`workspace-host`)

- [ ] 3.1 In `src/steps.rs`, add `latest_plans()`: the latest plan step of each unarchived session's latest run, read from the `steps` table. Verify with a test that covers two runs of one session (only the latest run counts), a run with no plan, and a plan step stored without counts.
- [ ] 3.2 Create `src/progress.rs` with the in-memory `plans` map on `Host` (set when a plan step is recorded, removed when a run starts, filled from `latest_plans()` on first use) and `Progress`, which holds the last board with a revision per entry and `refresh(&Host) -> bool`. Verify that unchanged entries keep their revision, that a changed plan bumps exactly the agent, its ticket and its project, and that a closed ticket changes its project's entry.
- [ ] 3.3 In `src/service.rs`, add the bounded `progress_changes` signal beside `step_changes`, and call `refresh` from `Actor::process` after an event that changed state and after a recorded plan step, after `schedule`. Verify beside the step-signal tests (`service.rs:4531`): a plan step sends one progress signal and no state signal; a command step sends none; a burst leaves one pending signal; a read followed by a change signals again.
- [ ] 3.4 Serve `Command::Progress { after }` with the entries whose revision is greater than `after`. Verify the cursor: `after: 0` returns the whole board, the returned revision returns nothing, and one plan change returns three entries.
- [ ] 3.5 Verify the spec's "Coordinator stays asleep" scenario: after a burst of plan steps from a worker, its coordinator's queue is empty, no message was written and no session's status changed.
- [ ] 3.6 In `tests/restart.rs`, verify the spec's "Restart" scenario: the board after a restart equals the board before it.

## 4. Desktop: board and bars (`workspace-desktop`)

- [ ] 4.1 In `src/bridge.rs`, expose `progress_changes()`. Create `src/progress.rs` with the client board (maps by id, merged from pages, pruning entries nothing refers to) and `watch_progress` / `fetch_progress` with one request in flight and a dirty flag, as `activity.rs:137-213` does for steps; start the watcher from `main.rs` where `watch_steps` starts. Verify the merge and pruning with unit tests, and by running the app that a plan step causes no `Command::Snapshot`.
- [ ] 4.2 In `src/ui.rs`, add `progress_bar(position, state, p)`, `segmented_bar(segments, p)` and the striped indeterminate track (`gpui::pattern_slash` if the pinned GPUI has it, otherwise slanted fixed divs), using `p.focus`, `p.green`, `p.red` and `p.edge`. Verify all four states in Tidal Light and Tidal Dark in the running app against `design/mockups/current/index.html`.
- [ ] 4.3 In `src/tree.rs`, add `agent_figure`, `ticket_figure`, `project_figure`, `strip_label` and `elapsed_label`. Verify with tests for every figure in the spec: "3 of 5", "working", "done", "passed", "failed", "blocked", "70%", "56% · 1 of 4 tickets accepted", "Step 4 of 6 · …", "6 of 6" for a finished checklist, and "50% · ticket 30%".

## 5. Desktop: the three placements (`workspace-desktop`)

- [ ] 5.1 In `src/conversation.rs`, add `progress_strip` at a fixed height directly under `conversation_header`: the project form for a coordinator (phase, segmented bar, figure, crew button) and the agent form for a worker (step label, bar, own percentage and the ticket's figure). Verify the spec scenarios "Coordinator selected", "Worker selected", "Worker with no plan" and "Steady layout" in the running app.
- [ ] 5.2 In `src/sidebar.rs`, add the 3 px track and the short figure to `project_row`, `ticket_row` and `session_row`, reserving the track on every unarchived row. Verify the scenarios "Rows under one project", "Blocked ticket row" and "Idle agent with no plan", and that a ticket row with a PR line still lays out correctly.
- [ ] 5.3 Create `src/crew_panel.rs` and add `Panel::Crew` in `src/main.rs`, opened by the strip's crew button: ticket cards with stage, percentage and bar, and agent rows with role and focus, model (`models::profile_label`), figure, elapsed time and bar, each linking to its conversation. Verify the spec scenarios "Open the crew", "Archived passed verifier", "Earlier cycle's verifiers" and "Accepted ticket".
- [ ] 5.4 Run the crew panel's 30-second elapsed-time timer only while the panel is open and a listed agent is working. Verify the "Idle panel" scenario in the running app.

## 6. Docs and validation

- [ ] 6.1 Update `docs/PRD.md` (the conversation and tree sections: the strip, the sidebar bars and the crew panel, and that progress is derived and never reported) and `docs/DEVELOPMENT.md` (the board, `Command::Progress` and the third signal). Verify that the text matches the specs.
- [ ] 6.2 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace`, `python3 scripts/test_communications.py` and `openspec validate --all --strict`, and paste the output in the report.
