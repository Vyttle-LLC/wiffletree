## Why

While an agent works, the user sees only a spinner. The provider adapters drop tool calls, commands, file edits, thinking and plans, and every interim text block is joined into the chat reply. That leaves the user blind during long turns and makes the chat noisy once the turn ends. The user needs to watch work as it happens and still keep the conversation focused on the messages that matter.

## What Changes

- Normalize each provider's streamed output into **work steps**: narration, thinking, commands, file edits, tool calls, searches and plan updates, each with a running, succeeded, failed or interrupted state.
- Stream steps live to the desktop app through a lightweight, cursor-based fetch instead of full snapshot reloads, coalesced to the PRD's 20–30 updates per second.
- Persist each step when it starts and ends. Bound its size. A restart shows what was running.
- Replace the chat's "is working…" row with a fixed-height **live activity card**. It shows the agent's latest narration line, which replaces the previous one like a ticker. Below that are the three most recent steps, plus the elapsed time and step count.
- Clicking the card opens a **turn history sheet** that slides in from the right. It shows the full live history of the turn: each narration paragraph with its tool steps grouped beneath it, and command output that expands on demand.
- When the turn ends, the card collapses to a **one-line summary** ("Worked 9m 12s · 23 steps") above the final reply. The summary opens the same sheet for that turn.
- **BREAKING (behavior):** the chat reply for a turn becomes only the turn's final text. Interim narration ("Let me check the tests…") moves to the Work stream. Interim text no longer appears in the transcript, and the reply is written when the turn ends instead of growing during it.
- Out of scope for this change: a team view that shows the live steps of a coordinator's child agents. This is recorded as a follow-up.

## Capabilities

### New Capabilities
- `work-stream`: capturing, persisting and presenting an agent's in-progress work, kept separate from the durable conversation.

### Modified Capabilities
<!-- None. Existing requirements live in the unarchived native-workspace-foundation change; this change adds a capability rather than editing them. -->

## Impact

- `workspace-host`: provider event parsing for Claude stream-json and Codex `exec --json` (`provider.rs`), step buffering and persistence (`service.rs`, new `steps` table), new `Steps` and `RunSummaries` commands, and a step-change signal beside the existing state signal.
- `workspace-core`: shared `Step`, `StepPage` and `RunSummary` types and their commands.
- `workspace-desktop`: the activity card and turn summary (`conversation.rs`), a turn history sheet (new module, using gpui-component's `Sheet`), and change handling and step fetching (`main.rs`, `bridge.rs`). The inspector is unchanged.
- Role instructions (`skills/*.md`): agents are told that only their final message reaches the chat.
- No new dependencies, and no extra model calls. A 1 Hz elapsed-time tick runs only while a visible session is working.
