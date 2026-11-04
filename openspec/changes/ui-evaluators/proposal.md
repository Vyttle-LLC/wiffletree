## Why

The UX eval-and-improve loop is paused because only the human can start its evaluators: fresh, blind agents on an exact model that read a prompt, drive the app under test and write a report. A coordinator should be able to start one itself, on the model the loop names, and be woken when it ends, without ever seeing what the evaluator thought.

On November 4, 2026 Michael approved `ui/mockups.html` and the plan behind it, with both screen-control modes: **App only**, which drives just the app under test through its automation socket, and **Full desktop**, which uses Claude's or Codex's own computer use.

## What Changes

- **Evaluator runs.** A new kind of launch, not a ticket agent and in no worktree. A run belongs to a project and has an id, a provider, model and effort, a mode, a run folder, a state (queued, waiting, running, finished, failed or stopped) and its times. Each run is a fresh provider session; its transcript is kept for the human.
- **Blind and isolated, enforced by the provider.** No shell, web, repository, brain, history, memory or workspace coordination tools, and none of the human's CLAUDE.md, AGENTS.md, settings, skills or MCP servers. It reads and writes only inside its run folder. Its first message is the folder's `prompt.md`, with nothing added. No Wiffletree session variable reaches it.
- **A pinned model.** A run uses only the provider, model and effort it was started with. An unknown model, an effort the model does not offer or a provider not signed in is refused at start. At a usage limit the run waits for the reset and says when; it never falls back.
- **Coordinator tools.** `start_evaluation(run_folder, provider, model, effort, mode)` returns the run's id; `list_evaluations` lists the project's runs; `stop_evaluation` stops one. The coordinator never receives a transcript or report text.
- **Off by default, per project.** The first `start_evaluation` in a project raises an Inbox card (screen 1) and holds the run. The human chooses App only, Full desktop or Not in this project; no agent can change it, and closing the project turns it off. Refusals name the reason.
- **Queue and screen lock.** App-only runs run side by side. Full-desktop runs share one queue for every project and one screen lock.
- **The completion wake-up** holds only the run's id, state, folder and duration.
- **The app driver** (ticket 3): an MCP tool over the automation socket and a test copy of the app launched with `~/ux-eval/launch.sh`'s isolation.
- **The desktop driver** (ticket 4): each provider's computer-use server through the helper app the spike chooses, Settings › Screen control, the Start-now card, Esc and Stop, and the menu bar indicator.
- **The UI** (ticket 5): the Agents panel's Evaluations group, the run page and the banners.

## Capabilities

### New Capabilities

- `ui-evaluators`: blind, model-pinned evaluator runs a coordinator starts, the human authorizes and watches, and Wiffletree isolates, queues and reports on.

### Modified Capabilities

None. Evaluator runs take no part in tickets, model selection or the agent tree.

## Impact

- `workspace-core`: `Role::Evaluator`, the evaluation records and commands.
- `workspace-host`: `evaluations.rs`, the evaluator launch in `provider/evaluator.rs`, the three coordinator tools, the Inbox card's answer, the scheduler, `coordinator.md`.
- `workspace-desktop`: the Inbox card (generic options until ticket 5), the Evaluations group and the run page (ticket 5), Settings › Screen control (ticket 4).
- `docs/FEATURES.md`, `design/README.md`.
- Older stores load unchanged: the new tables start empty and a project with no choice is off.
