## 1. Ticket 1 · Desktop computer-use spike

- [ ] 1.1 Claude `-p` with `claude --computer-use-mcp` through `--mcp-config`: tools, app access, lock.
- [ ] 1.2 Codex `exec` with its computer-use server enabled through `-c`.
- [ ] 1.3 Which process macOS asks for Screen Recording and Accessibility, and a helper app that alone holds them.
- [ ] 1.4 Stop and Esc for each provider. Findings in `docs/computer-use-spike.md`.

## 2. Ticket 2 · Evaluator runs and `start_evaluation`

- [x] 2.1 Core: `Role::Evaluator`, the evaluation records, `Command::SetEvaluatorPermission`, `Command::StopEvaluation`, `Command::Evaluations`.
- [x] 2.2 Host: the `evaluations` and `evaluator_permissions` tables; a run's session outside the agent tree, archived when the run ends.
- [x] 2.3 The isolated launch for Claude and Codex, its start and tool-call checks, the cleared environment and Codex's own home.
- [x] 2.4 The model check against detection and the providers' listed models; waiting at a limit with the reset time.
- [x] 2.5 `start_evaluation`, `list_evaluations` and `stop_evaluation`, coordinator only; desktop refused as not available yet.
- [x] 2.6 The Inbox card and its answer; archiving the coordinator turns evaluators off.
- [x] 2.7 The admission queue and the screen lock.
- [x] 2.8 The completion message; the human's stop.
- [x] 2.9 `coordinator.md`, `docs/FEATURES.md`, the tool descriptions.
- [x] 2.10 Tests: launch arguments, start checks, refusals, waiting without fallback, the completion message, the queue, the tool contract.

## 3. Ticket 3 · App driver and test target

- [ ] 3.1 An MCP driver over `workspace-desktop --automation-socket`: screenshot, click, type, key, scroll, drag, resize; offered to app runs only, through the run's strict MCP configuration.
- [ ] 3.2 The app captures its own window in-process if it can, so no Screen Recording grant is needed.
- [ ] 3.3 The host launches the test copy with `launch.sh`'s isolation: a disposable profile, fake provider CLIs, an empty `GH_CONFIG_DIR`, no workspace variables, and a switch that stops login-shell PATH adoption.

## 4. Ticket 4 · Desktop driver

- [ ] 4.1 Each provider's computer-use server connected to desktop runs through the spike's helper app; `computer_use` on for those Codex runs only.
- [ ] 4.2 Settings › Screen control: live permission and provider checks (screen 2).
- [ ] 4.3 The Start-now card between taking the screen lock and starting (screen 3); Later and Skip.
- [ ] 4.4 Esc and Stop, and the menu bar indicator while a run holds the screen.
- [ ] 4.5 Lift the refusal of desktop runs.

## 5. Ticket 5 · UI

- [ ] 5.1 The Agents panel's Evaluations group with every state (screen 4).
- [ ] 5.2 A run's page with its model, mode, folder, steps and Stop (screen 5).
- [ ] 5.3 The banners, the Inbox card as drawn (screen 1) and the project menu's UI evaluators choice.
- [ ] 5.4 Archive this change.
