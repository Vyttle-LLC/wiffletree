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

- [x] 3.1 An MCP driver over `workspace-desktop --automation-socket`: screenshot, click, double-click, right-click, move, type, key, scroll, drag, resize and window; offered to app runs only, through the run's strict MCP configuration, and allowed by the start and tool-call checks.
- [x] 3.2 The app captures its own window in-process (`capture`, at points or native scale), measured to need no Screen Recording grant; `resize` keeps the window on its display.
- [x] 3.3 The host launches the test copy with `launch.sh`'s isolation: a disposable profile, fake provider CLIs, an empty `GH_CONFIG_DIR`, no workspace variables, and `WORKSPACE_SKIP_LOGIN_ENV`, which stops login-shell PATH adoption. It waits for the socket, stops the copy's whole tree when the turn ends, and refuses a second live run on the same data folder.

## 4. Ticket 4 · Desktop driver

The spike ruled out each provider's own computer use; the human chose Wiffletree's own driver in a helper app.

- [x] 4.1 Wiffletree Evaluator (`crates/workspace-evaluator`): its socket protocol, the allowlist and hit-test rules as pure functions, ScreenCaptureKit screenshots of the allowed apps alone, input posted only over their windows and keys only to an allowed frontmost app, open_app and list_windows.
- [x] 4.2 Bundled in `Contents/Helpers` and signed with the app by `scripts/package_macos.sh`; development signing in `docs/DEVELOPMENT.md`.
- [x] 4.3 `wiffletree-host desktop-driver-mcp`, the `desktop` server, as Claude's and Codex's only MCP server, with the app driver's start and tool-call checks; Codex keeps `--ignore-user-config` and `computer_use` off.
- [x] 4.4 `apps` in `start_evaluation`; the refusal of desktop runs lifted.
- [x] 4.5 The Start card between taking the screen lock and starting (screen 3): Start now, Later and Skip these runs; a desktop run at its limit ends.
- [x] 4.6 The helper's grants checked before the provider starts; Esc, the menu bar item's Stop and the helper quitting stop the run; the helper quits, or is killed, when the turn ends.
- [ ] 4.7 On-screen check with the human: the signed helper's grants, a Calculator run for Claude and for Codex, Esc and Stop.

## 5. Ticket 5 · UI

- [ ] 5.0 Settings › Screen control (screen 2): the helper's Screen Recording and Accessibility rows, read through the helper; the Providers rows no longer apply.
- [ ] 5.1 The Agents panel's Evaluations group with every state (screen 4).
- [ ] 5.2 A run's page with its model, mode, folder, steps and Stop (screen 5).
- [ ] 5.3 The banners, the Inbox card as drawn (screen 1) and the project menu's UI evaluators choice.
- [ ] 5.4 Archive this change.
