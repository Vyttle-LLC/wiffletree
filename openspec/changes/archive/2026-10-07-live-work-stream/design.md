## Context

See proposal.md for the motivation. These parts of the current code shape the approach:

- **Adapters** (`workspace-host/src/provider.rs`). Claude runs `claude -p --output-format stream-json --include-partial-messages`. Codex runs `codex exec --json`. Each adapter emits `ProviderEvent::Text` only for completed assistant text. Claude `tool_use`, `tool_result` and `thinking` blocks are dropped, along with Codex `command_execution`, `file_change`, `mcp_tool_call`, `reasoning`, `todo_list` and `web_search` items. Claude `stream_event` deltas go only to usage telemetry.
- **Host actor** (`service.rs`). `ProviderEvent::Text` is appended to `Active.output` and upserted as the `output:{run}` message on every block. That is why interim narration ends up in the reply. `provider_runs` already records `started_at`, `finished_at` and the outcome, and the startup code marks unfinished runs as interrupted.
- **Desktop** (`main.rs`, `bridge.rs`). `Service::changes()` is a `Receiver<()>`. Every notification triggers a full `Command::Snapshot`, and the transcript is reloaded as a message page. The trailing "is working…" row is a synthetic list row (`transcript_rows`).
- **Overlays.** gpui-component 0.7.1 provides `Sheet` (a panel that slides in from the right over the window), `shimmer` and `collapsible`. The app already uses its `list` for virtualized rows. The inspector's `Events` panel shows the coarse `activity` table and stays as it is.
- **PRD constraints.** Batch display deltas at 20–30 per second. Never drop durable messages for streaming. Routine tool output must not wake models. Use no idle timers. Use virtualized lists. Store raw output with an explicit retention policy.

## Goals / Non-Goals

**Goals:**
- One provider-neutral step model that both adapters fill in.
- A streaming path that costs nothing per delta beyond memory, and nothing at all when idle.
- Keep progress inside the conversation. Show detail only on demand, with no new pane and no layout changes.

**Non-Goals:**
- A team view that shows the steps of a coordinator's child agents. That is a follow-up built on the same `Steps` command.
- Showing full tool inputs and outputs, or diffs. Details are bounded summaries. The Changes and Git views remain the place to see full diffs.
- Searching or filtering steps.
- Sending steps to other agents, or storing them in the project brain.

## Decisions

### 1. Step model in `workspace-core`

```rust
pub struct Step {
    pub run_id: String,
    pub id: String,           // provider item / tool_use id, or "narration:<n>"
    pub parent_id: Option<String>, // Claude subagent work nests under its Task tool step
    pub kind: StepKind,       // Narration | Thinking | Command | FileEdit | Read | Search | Plan | Tool
    pub state: StepState,     // Running | Succeeded | Failed | Interrupted
    pub title: String,        // ≤ 200 chars, single line
    pub note: Option<String>, // ≤ 40 chars beside the title: "+12 −4", "exit 1", "2 files"
    pub detail: Option<String>, // ≤ 4 KiB, keeps the tail
    pub omitted: u32,         // bytes dropped from detail
    pub seq: u32,             // order of first appearance within the run
    pub revision: u64,        // per-session monotonic, set by the host
    pub started_at: i64,
    pub finished_at: Option<i64>,
}
```

The adapter emits `ProviderEvent::Step(StepUpdate)`, a full snapshot of what it knows about the step. The host assigns `seq`, `revision` and times, and replaces the existing step with the same `(run_id, id)`. Narration and thinking are the agent talking; every other kind is an *action*, and the step counts shown to the user count actions only.

*Alternative considered:* separate event variants per kind (`ToolStarted`, `ToolFinished`, …). Rejected because every consumer would need to match on all of them, while one upsert type keeps the host and UI generic.

### 2. Provider mapping

| Provider event | Step |
|---|---|
| Claude `stream_event` `content_block_start` (text / thinking) + `*_delta` | Narration / Thinking, Running; deltas append to the title and detail |
| Claude `assistant` block `tool_use` (or its `content_block_start`, so a tool shows while its input streams) | `Bash` → Command (title = command); `Read` → Read; `Edit`/`Write`/`MultiEdit` → FileEdit (title = path relative to the turn's directory; `Write` notes `+N`); `WebSearch`/`WebFetch`/`Grep`/`Glob` → Search; `TodoWrite` → Plan; `Agent`/`Task` → Tool ("Agent · description"); `mcp__server__tool` → Tool (the workspace's own tools by name alone). Running |
| Claude `user` block `tool_result` | Same id → Succeeded, or Failed if `is_error` (note `exit N` from "Exit code N"); output → detail; an edit's `structuredPatch` notes `+a −b` |
| Claude events with `parent_tool_use_id` | Subagent tool calls arrive only as complete messages; `parent_id` = that id. Subagent text is not narrated |
| Codex `item.started` / `item.updated` / `item.completed` | `command_execution` → Command (title without the `/bin/zsh -lc` wrapper, output, `exit N` when nonzero); `file_change` → FileEdit (paths, `N files`); `mcp_tool_call` → Tool; `web_search` → Search; `todo_list` → Plan (`n of m done`); `reasoning` → Thinking; `agent_message` → Narration |

Text blocks close as Succeeded when the block ends. Plans are statements, so they are always Succeeded. Claude currently redacts thinking text (only signatures stream), so empty thinking produces no step. When partial messages are unavailable, a complete text block that was never streamed still becomes narration. Unknown events are ignored, as the spec requires. Recorded and synthetic streams for both providers live in `crates/workspace-host/tests/fixtures/streams/`.

### 3. The final reply comes from the adapter's terminal event

The adapter no longer emits `Text` for every block. It emits `ProviderEvent::Reply(String)` once, from Claude's `result.result` (or its last text block when `result` is absent) or from the last Codex `agent_message` before `turn.completed`. A failed turn sends no reply; its error is shown instead. The host writes `output:{run}` once when the turn finishes, and the parent notification uses the same reply. `Active.output` keeps its role for that notification.

*Alternative considered:* the host promotes the last Narration step. Rejected because each provider already reports its final text, so the adapter can report the reply directly.

### 4. Host buffering and persistence

- The new table `steps(session_id, run_id, id, revision, data, PRIMARY KEY(run_id, id))` has an index on `(session_id, revision)`.
- Each running session keeps an in-memory `steps: Vec<Step>` in `Active`, plus a per-session `revision` counter. The counter is seeded from `MAX(revision)` when the session is first seen.
- Deltas mutate only the in-memory step and bump `revision`. The step is written to SQLite on **start** and on **state change**. Text deltas are written only when the block closes. The write rate is therefore a few rows per tool call, never one row per token.
- Bounds: 2,000 steps per run. After that, steps are counted in `provider_runs.detail.omitted_steps` and not stored. Details are capped at 4 KiB, keeping the tail.
- Startup reconciliation, which already exists for `provider_runs`, also sets `state = Interrupted` on any stored step still marked Running.
- Retention: on host start, delete steps whose run finished more than 30 days ago. This is one indexed `DELETE`.

### 5. A separate step signal and a cursor fetch

`Service::changes()` stays a `Receiver<()>` for state, and `Service::step_changes()` adds a second `Receiver<()>` for steps. Both are bounded to one pending signal, which coalesces any number of changes until a client reads it. A single typed channel would have let a pending step signal hold back a state change, so the two stay separate. Step events no longer mark state as changed, so streaming never triggers a snapshot reload.

A new `Command::Steps { session_id, after: u64, run_id: Option<String>, limit }` returns `{ steps, revision }`. These are the in-memory steps with `revision > after` merged with stored ones. With `run_id` set, it returns that run's steps in `seq` order for the history sheet. `Command::RunSummaries { run_ids }` returns `{ run_id, started_at, finished_at, steps, failed }` for the `output:` replies on a loaded transcript page. That costs one indexed query per page, not one per message.

**Pacing happens on the client.** The desktop keeps at most one `Steps` request in flight. A signal that arrives during a request sets a dirty flag, and when the request completes the client fetches once more, no sooner than 33 ms after the previous fetch started. That one-shot delay exists only while changes are pending, so an idle app runs no timer. A host-side throttle was rejected because, without a timer, it could hold back the last change of a burst until the next event.

*Alternative considered:* push step payloads over the change channel. Rejected because a slow UI would make an unbounded channel grow, while pulling by cursor bounds memory naturally.

### 6. UI placement: card, sheet, summary

- **Activity card.** The existing trailing working row in `conversation.rs` becomes a fixed-height card with four parts:
  - a header with the agent's name, a spinner, the elapsed time (`m:ss`) and the step count
  - a one-line headline: the latest narration text, ellipsized and shimmering while it streams
  - three step rows: the latest non-narration steps, each with a state glyph, a kind icon and a monospace title, with the duration or line counts on the right
  - a "View all" affordance; the header and body open the history sheet
  - a chevron toggle that minimizes the card to its header line, which then shows the current step (narration or step title) after the step count. Toggling animates the height and changes no other layout. The choice is `minimize_activity: bool`, which applies to every session. The desktop has no persisted preferences yet (appearance resets on launch), so this adds a small `desktop-preferences.json` in the app's data directory. It is read at launch and written when the choice changes. It is client-side on purpose: a future remote client keeps its own view preferences, not the host's.

  Fixed heights for the full and minimized states keep the virtualized list's measurements stable, so streaming never shifts the reader's position. A 1 Hz `cx.spawn` timer updates the elapsed time only while the selected session is `Working`, and stops when that changes.
- **Turn history sheet.** A new `history_sheet.rs` opens a `Sheet` from the right, about 560 px wide, for one `run_id`. It renders the run's steps in `seq` order. Each Narration step is a Markdown paragraph, using the existing `markdown()` helper, and each following non-narration step is a compact row indented beneath it. A step with detail expands in place when clicked, and subagent steps are indented under the call that started them. The list follows the tail unless the reader has scrolled up, using the same rule as the transcript. Because a channel signal goes to only one receiver, the workspace's step watcher forwards each signal to the open sheet instead of the sheet listening on its own. Escape and outside clicks close it, and nothing in the composer changes.
- **Finished turn summary.** For a message whose id is `output:{run}`, the transcript renders a one-line `▸ Worked 9m 12s · 23 steps`, with `· 1 failed` added when it applies, directly above the reply. The data comes from `RunSummaries`. Clicking it opens the sheet for that run. While the turn is running, the card takes the place of the summary, and the reply row does not exist yet.

*Alternatives considered:*
- A Work panel in the inspector. Rejected because it competes with Git and the other panels, needs auto-open rules, and splits the user's attention between two panes.
- An inline expandable trace inside the transcript. Rejected because expanding it would make the conversation long and noisy again, which is the problem we're solving.
- A popover anchored to the card. Rejected because it is too small for long histories and closes on the first outside click while the user is reading.

### 7. Agent instructions

`skills/communication.md` gains one line: only your final message reaches the user's chat, so put the answer or summary there.

## Risks / Trade-offs

- [A final reply could be short while the substance is in earlier narration] → The instruction line, plus the reply's step-summary link to the full narration.
- [The provider stream formats are not versioned contracts] → Mapping lives in one function per provider with fixture tests built from recorded JSON lines. Unknown shapes degrade to no step, never to a failed turn.
- [Claude thinking may be redacted or summarized] → Thinking steps are optional, and an empty thinking block shows as "Thinking" with no detail.
- [Interim text vanishing from chat may surprise users of existing sessions] → Old `output:` messages are left untouched, and only new turns use the final-reply rule.
- [Step writes add SQLite load during eight active streams] → Writes happen only on starts and state changes, and will be measured with the existing benchmark fixture extended for steps.

## Migration Plan

- An additive `steps.sql` with `CREATE TABLE IF NOT EXISTS`, run at every open the same way `usage.sql` is (`lib.rs`). No existing data changes.
- Rollback: older builds ignore the `steps` table, and replies written as a single final text read correctly in older builds.

## Open Questions

- Should the 30-day step retention and 4 KiB detail cap be user-configurable? Defaults are fine to ship with, and settings can be added later without changing the design.
