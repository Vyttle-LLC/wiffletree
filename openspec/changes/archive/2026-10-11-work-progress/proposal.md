## Why

The tree shows whether an agent is working, blocked or done, but not how far along anything is. To judge a project, the human has to open each agent's activity card and read its plan, then work out from ticket states what is left. The Claude Code plugin "Savvy Progress" answers this with a progress bar and a crew panel, but it has agents self-report through extra MCP tools. Wiffletree already records everything needed: each agent's plan steps, each session's status, each ticket's state and its verification record. This change turns those records into progress bars at three levels (agent, ticket and project). The one thing it asks of an agent is a checklist of its own steps, because the Claude CLI the host runs offers no plan tool on Opus or Sonnet (see design.md).

## What Changes

- **Derived progress.** The host works out progress from plan steps, session status, ticket state and the verification record. No agent reports a percentage or a stage.
- **One checklist tool.** `set_plan` on the workspace tool server lets any agent record its checklist, which becomes a plan step like a provider's own. A short instruction in the communication contract asks every agent to keep one. It is display only: it wakes no one and never replaces `report`.
- **Agent progress.** Checklist items done out of total, from the latest plan step of the agent's current run, with the first unfinished item as its current step. Working with no plan shows a striped bar and the word "working", never a made-up percentage. A passed verifier or a finished agent shows 100%.
- **Ticket progress.** A stage filled in by the ticket's agents: Implementing 0–60% (the implementer's checklist), Verifying 60–90% (verifiers passed out of the cycle's verifiers), Awaiting acceptance 90%, Accepted 100%. A blocked ticket keeps its position and turns red.
- **Project progress.** The equal-weight average of its tickets, drawn as one bar with a segment per ticket, with a phase label and "N of M tickets accepted". With no tickets it reads "Planning" and uses the coordinator's own checklist.
- **Three placements.** A strip pinned under the selected conversation's title; a thin bar and short figure under every project, ticket and agent row in the sidebar; and a crew button on the coordinator's strip that opens a crew panel listing each ticket with its stage and each of its agents with model, checklist count and elapsed time.
- **Structured plan counts.** A plan step carries its counts and current item as data. Today they exist only inside the title "Plan · 1 of 3 done".
- **Incremental delivery.** The host signals only when a figure changed, and the desktop then fetches only the changed entries. Progress never wakes, notifies or messages an agent, and never triggers a snapshot reload per step.
- **Not included:** mapping Claude's `TaskCreate`/`TaskUpdate` tools, cost or token estimates, time-remaining estimates, progress in `workspace_context` or in any message to an agent, weighting tickets by size, and progress history.

## Capabilities

### New Capabilities
- `work-progress`: deriving agent, ticket and project progress from existing records; the `set_plan` checklist tool; the four bar states; the progress strip, the sidebar bars and the crew panel; incremental delivery that wakes no agent.

### Modified Capabilities
- `work-stream`: a plan step carries its done and total counts and its first unfinished item as data, beside its title, and can come from the `set_plan` tool as well as a provider's plan.

## Impact

- `workspace-core`: `steps.rs` (`PlanCounts`, `Step.plan`); a new `progress.rs` with the progress types and the pure derivation; `lib.rs` (`Command::Progress`, the module export).
- `workspace-host`: `stream.rs` (`plan()` fills the counts for Claude `TodoWrite` and Codex `todo_list`); `steps.rs` (the latest plan per session, rebuilt from stored steps); a new `progress.rs` (the board, its revisions and the changed-entries page); `service.rs` (refresh after state changes and plan steps, a third bounded signal, `Command::Progress`, the `set_plan` call); `mcp.rs` (the tool); `skills/communication.md` (the checklist instruction, version 6).
- `workspace-desktop`: `bridge.rs` (the progress signal); a new `progress.rs` (the client's board and its fetch); `ui.rs` (the bar drawings); `tree.rs` (figure and label text); `conversation.rs` (the strip); `sidebar.rs` (bars on project, ticket and agent rows); a new `crew_panel.rs` and `main.rs` (`Panel::Crew`).
- No new table, no SQL migration, no new dependency and no model calls. Steps are stored as JSON, so the new field needs no schema change.
- Docs: `docs/PRD.md` and `docs/DEVELOPMENT.md`, updated by the implementing ticket.
- Mockup: `design/mockups/current/source.html` (strip, sidebar bars and crew panel, simulated).
