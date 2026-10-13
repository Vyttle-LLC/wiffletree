## Context

See proposal.md for why. The current behavior, on main e62b305, which includes `stop-leftover-provider-processes` (PR #46):

- `workspace-desktop/src/context_view.rs`: `context_chip` draws the header chip and `context_card` the Context card. `sync_context` measures again when the selected session's `last_finished_at` is newer than what the card shows and the session is not working. The measurement is `workspace_host::context::usage`, which runs `claude -p /context …` with `--no-session-persistence`: the existing precedent for a bare slash command.
- `workspace-host/src/service.rs`: `schedule` starts a turn for each session with due input unless it is paused, disconnected, held or already in `active`. `start_turn` writes a `provider_runs` row, marks the input delivered and builds the prompt with `turn_prompt`, which wraps every message. The `ProviderEvent::Finished` arm settles the run, appends the reply, completes or holds the input, and messages the parent. `check_in_by_turns` counts a session's `provider_runs` rows since its parent's last message.
- Around a host stop, also `service.rs`: every run records its provider's process group and that leader's start time (`ProviderEvent::Spawned`). `stop` cancels each running turn and waits up to `QUIT_WAIT` for its provider thread to report the end, which the thread does only after stopping and reaping the group. At startup `recover_unfinished_turns` stops the group of each unfinished run while its leader still has the recorded pid and start time (`provider::stop_leftover`), marks the run `interrupted` and its session `Disconnected`, and raises a `leftover-process:` inbox note for a group it could not confirm. `hold_reason` holds a session's turn while such a note is open and its group runs.
- `workspace-host/src/provider.rs`: `input` is what the provider reads on stdin; `configure` builds the arguments. In `run`, the Claude `init` arm fails the run unless the `agent_workspace` MCP server is connected, the `result` arm takes `value["usage"]`, and `system` records other than `init` are ignored.
- `workspace-host/src/usage.rs`: `record_usage(run, request, model, usage, complete, at)` writes one `usage_requests` row per provider request, keyed by a run. Claude's rows come from `stream_event` records during a turn.
- `SessionRuntime` (`workspace-core/src/lib.rs`) is stored as JSON in `runtimes.data` and reaches the desktop in the snapshot.

Verified on throwaway sessions with Claude Code 2.1.295 (the ticket brief has the full records):

- A stdin prompt that is exactly `/compact`, or `/compact <instructions>`, to `claude -p --output-format stream-json --verbose --resume <id>` runs the compact command. The session id does not change.
- The stream is, in order: a `system` `status` record with `"status":"compacting"`; a `system` `status` record with `"compact_result":"success"`; a `system` `init`; a `system` `compact_boundary` with `compact_metadata.pre_tokens` and `post_tokens`; replayed `user` records holding the summary; a `result` with an empty `result` and `num_turns` 0.
- The `result` record's `usage` is all zeros. The tokens and cost are in `modelUsage` and `total_cost_usd`, which count the session's whole life, not this run: after three turns and a compaction they are the four runs' sum.
- A session with nothing new gives `"compact_result":"failed","compact_error":"Not enough messages to compact."`, exit code 0 and `is_error:false`.
- `codex exec … resume` has no compact command.

## Goals / Non-Goals

**Goals:** one human action that compacts a Claude agent now or right after its running turn; a request that is never lost and never runs unseen; an outcome read from what Claude reported, not from the exit code; the compaction's tokens in the ledger; no effect on parents, check-ins or message delivery beyond the ordering.

**Non-Goals:** a coordinator or agent tool; Codex compaction (`codex app-server`'s `thread/compact/start` is a separate route); changing or recording Claude's automatic compaction; a cost column or any new Usage page UI; summarizing conversations for the transcript.

## Decisions

### 1. The request lives on the session's runtime
`SessionRuntime` gains two fields, both `#[serde(default)]` and skipped when empty:

- `compaction: Option<Compaction>` with `instructions: Option<String>`, `requested_at`, and `state`: `queued` or `running`.
- `last_compaction: Option<CompactionOutcome>` with `result` (`compacted`, `nothing_to_compact`, `failed` or `interrupted`), an optional `reason`, and `at`.

A session has at most one request. Because the runtime is already persisted and already in the snapshot, the request survives a restart and reaches the card without a new table or a new query.
*Alternative:* a queued message with a reserved id, so the existing queue orders it. Rejected because a message is input to a turn: it would need exceptions in delivery, receipts, the waiting count and `MAX_TURN_MESSAGES`.

### 2. Three commands, human only
- `CompactContext { session_id, instructions }` stores a `queued` request. It is refused, with the reason, when the session's provider is not Claude, when it has no provider session id, when it is archived, or when it already has a request. `instructions` is trimmed, empty becomes `None`, line breaks become spaces so the command stays one line, and more than 1,000 characters is refused.
- `CancelCompaction { session_id }` removes a `queued` request. It is refused once the request is `running`.
- `DismissCompaction { session_id }` clears `last_compaction`.

They are client commands only. `mcp.rs` gains no tool, so no agent or coordinator can call them. Each records an event (`compaction_requested`, `compaction_cancelled`), which the Events panel already lists.

### 3. One rule for when it runs
A queued compaction starts when a turn for that session could start, and before any input is taken:

- In `schedule`, before `admit_rounds` and the message pass, each session with a `queued` request is started if it is not in a turn, not paused or disconnected, not archived, has no held input, its project is live, and `hold_reason` is `None`: its provider is enabled and no leftover provider process holds it. Held input is the message pass's own condition (`NOT EXISTS … receipt='held'`); a session can be Ready with input still held, and it is reconciled before anything runs. A request does not need due input, so an idle agent compacts at the next pass, which the command triggers.
- No turn starts for a session that has a request, queued or running. Turns start in two places, and both check it. The message pass skips the session at the point where it would start the turn, after the `hold_reason` check, so a session whose turn is held, such as one whose provider is disabled in Models, is still blocked and reported to its parent by `hold_turn` exactly as without a request. `admit_round` treats a member with a request like a paused member, so the round waits whole and is admitted on the pass after the compaction settles; a member whose turn is held anyway does not make the round wait, and is held and reported in the round as today. `start_turn` also refuses such a session, so a path added later cannot run a turn and a compaction on one Claude session at once. Waiting messages, a round's message included, stay `queued` and are delivered by the first turn after the compaction settles.
- The worktree gate and an unadmitted round do not delay the compaction itself. It runs no tools and writes no files in the worktree.

What follows from the rule, with nothing extra to build:

| Situation | What happens to a queued request |
|---|---|
| The turn ends normally | It runs at once, then the waiting messages are delivered. |
| The turn fails, or a parent or the human stops or pauses the agent | The session is paused or disconnected, so the request waits, still shown and still cancellable. It runs when the agent is resumed, before the retried or new input. |
| The project is stopped | It waits the same way until the project runs. |
| The host restarts | The request is in the runtime, so it is still queued. The interrupted turn leaves the session disconnected, so it waits as above. |
| Claude is disabled in Models | It waits, like the agent's turns. With input waiting, the agent is blocked and its parent told, as today. |
| A leftover provider process holds the agent | It waits until the process exits or its inbox note is dismissed, like the agent's turns. |

A request therefore never disappears and never runs while the person cannot see it: the chip and the card show it for as long as it waits.
*Alternative:* cancel the request when the turn it waited on does not end normally. Rejected because it loses a request the person made.

### 4. A compaction run is not a turn
`start_compaction` in `service.rs` is separate from `start_turn`:

- It writes a `provider_runs` row with `messages` `[]` and `detail.kind` `"compaction"`, because `usage_requests.run_id` references a run and an open run already defers worktree removal.
- It sets the request `running`, keeps the session's stored status as it is, and tracks the process in a new `compacting` map beside `active`. It does not touch `active`, `in_turn`, the human turn count or any message receipt.
- A compaction's row is not a turn's row. One shared SQL condition, `IS_TURN_RUN` (`detail.kind` absent), goes into every query that means "this session's turns": the count in `check_in_by_turns`; the `MAX(started_at)` in `stopped_by_parent_note` and in `resume_note`, so a compaction that ran after a stopped or interrupted turn does not hide that turn's note from the next prompt; the latest-run lookups in `steps.rs` (`steps` without a run id, and the plan read for progress); and the unreported-run count in `usage.rs`. `check_in_long_turns` walks `active`, so it never reports a compaction.
- When it finishes, the host settles the run, records the outcome (decision 6), clears the request, writes the conversation line (decision 8) and sets `last_compaction`, all in one store unit, so a host that dies there leaves an unfinished run with its request for the next start, never a finished run beside a request that still runs. It appends no reply and sends the parent nothing.

**Stop and Pause while it runs.** A compaction that has started is not interrupted: a human Pause or Stop, a parent's `stop_agents` and a project Stop still set the session's status, and the compaction finishes. No turn follows, because the session is then paused.

**Host shutdown.** A compaction is stopped at a quit the way a turn is. `stop` sets the cancel flag of every entry in `compacting`, as it does for `active`, and its one bounded wait (`QUIT_WAIT`) covers both maps. A cancelled compaction's provider thread stops and reaps its process group before it reports the end, exactly as a turn's does, and `compaction_finished` then settles the compaction as `interrupted` (decision 10) with the run marked `process_gone`. Only a shutdown ever sets a compaction's cancel flag, so a cancelled end always means the quit. A compaction that outlasts the wait stays unfinished, its request still `running`, for the next start.

**Status.** The stored status does not change, so an agent that was Blocked is Blocked again afterwards. The desktop draws "Compacting" from `compaction.state` (decision 9). `send_message` to a compacting agent returns the plain `queued` receipt.

### 5. The bare command, with a turn's arguments
`Turn` gains a kind: a prompt, or a compaction with its optional instructions. For a compaction, `provider::input` returns exactly `/compact` or `/compact <instructions>` as plain text, with no wrapper and no attachments, and `configure` adds no `--input-format`.

Every other argument is the same as a turn's: model, effort, the appended role instructions, `--resume`, and the `agent_workspace` MCP configuration with a fresh token that is never registered in `active`. So the `init` check in `provider::run` is **satisfied, not bypassed**, and stays one unconditional check. A tool call under that token is refused by the existing token lookup; a compaction makes none.
*Alternative:* the context probe's empty MCP configuration, skipping the `init` check for compactions. Rejected for now because it changes the tool list the summary request is built from. If that misses the prompt cache the turns built, an 800k-token compaction is billed as fresh input. Task 2.1 measured both on throwaway sessions (Claude Code 2.1.295, `claude-sonnet-5-5`, three turns then `/compact`), taking each compaction's own tokens as what `modelUsage` rose by: 30,840 cache-read tokens with a turn's arguments and 30,885 with the empty configuration, on contexts of 32,560 and 32,576 tokens. Each is exactly what that session's last turn read from the cache, so neither misses it. The turn's arguments are kept anyway: they need no branch in `configure` and none in the `init` check.

### 6. The outcome is read from the status record
`provider::run` gains two Claude arms and emits one new event, `ProviderEvent::Compaction`:

- `system` with subtype `status` and a `compact_result` field: `success`, or `failed` with `compact_error`.
- `system` with subtype `compact_boundary`: `pre_tokens` and `post_tokens`.

The host maps them in this order:

1. `compact_result` is `success` → **compacted**. The figures come from the boundary record; if it is missing, the line has none.
2. `compact_result` is `failed` and `compact_error` starts with "Not enough messages to compact" → **nothing to compact**, with Claude's text as the reason.
3. Anything else → **failed**, with the first of: `compact_error`, the `result` text when `is_error` is true, the existing "exited without a successful terminal result" text. This covers a missing status record, a nonzero exit and a process that could not start.

The exit code and `is_error` never decide success. If a later CLI rewords "Not enough messages", case 2 falls into case 3: louder than intended, never wrong.

### 7. Ledger rows from `modelUsage`
A compaction streams no request events and its result record's `usage` is zero, so `modelUsage` is the only place its tokens appear. But `modelUsage` counts the session's whole life by model: every result record of a resumed session repeats all earlier turns, whose requests are already in the ledger from their stream events. So each Claude result record is treated as a reading of a counter, as Codex's cumulative usage already is:

- `provider::run` emits `ProviderEvent::ModelUsage` for every Claude result record, a turn's and a compaction's.
- `Host::record_model_usage` keeps the latest reading per Claude session in `usage_counters` (scope `claude:<session>`, counts by model, with `inputTokens`, `outputTokens`, `cacheReadInputTokens` and `cacheCreationInputTokens` mapped to the names `usage::normalize` reads).
- A turn's reading only replaces the stored one. A compaction's also records what each model's counts rose by since the reading before it, as one request with id `compact:<run>:<model>`, `complete` true. `record_usage` then attributes it to the agent and its coordinator as for any request.
- A reading is a baseline only when the session's run immediately before the compaction took it; `usage_counters` stores the run with the counts. A turn that is stopped, paused, killed by a quit or that exits without a result record sends no `modelUsage`, but Claude still adds its requests to the session's total and the ledger already has them from stream events; a compaction after it has no baseline. So does one with no earlier reading at all, such as a session whose turns all ran before this change. Without a baseline, or when a count went down, nothing is recorded, and the compaction's own reading still becomes the latest. Under-counting that one compaction is accepted; counting a turn twice is not. A rise of zero records nothing either.
- A model the baseline does not name starts from nothing: on a throwaway session a result record kept listing, unchanged, a model only an earlier run had used. A record with a model whose counts cannot be read is no reading at all, so no model is ever left out of one and later taken for new.
- The reading and its requests are written in one `atomically` unit, so a failed insert leaves the reading where it was and nothing is lost.

`modelUsage` and `total_cost_usd` are kept in the run's `detail.result` as Claude reported them, so they are the session's totals; the ledger has no cost column and gains none. An empty or missing `modelUsage` records nothing. Turns keep reading `usage`.
*Alternative:* record `modelUsage` as it is. Rejected because it counts every earlier turn again: on a throwaway session of three turns it reported 90,110 cache-read tokens for a compaction that read 30,840.

### 8. The conversation line
The host writes one message from the session to itself with id `compaction:<run>` and receipt `completed`. The prefix is new. Replies, stop notices and errors are all written by `append_output` under `output:…` ids, which the conversation draws as the agent's reply; the compaction line is not written through `append_output`, so it is never taken for a reply, by the conversation or by the check-in's "Last reply" lookup. A message a session sends itself is not turn input and wakes nobody.

| Outcome | Body |
|---|---|
| compacted | `Context compacted · 812k → 5k`, plus a second line `Kept: <instructions>` when some were given |
| failed | `Compaction failed · context unchanged` |
| interrupted (decision 10) | `Compaction interrupted` |
| nothing to compact | no message |

`conversation.rs` draws a `compaction:` message as a one-line entry at the turn summary's indent, never as a bubble, and red for the two failure bodies. The token formatter moves from `context_view.rs` to `workspace-core` as `compact_tokens`, so the line and the card print figures the same way. The figures are Claude's `pre_tokens` and `post_tokens`. The second counts only the summary, not the system prompt and tools the card also counts, so it reads lower than the card's new total: the mockup's line says 5k beside a card at 41k.

### 9. Desktop
All of it matches `design/mockups/current/compact-context.html`.

- **Row** (`context_card`): a hint and a button under the card's body. Claude with a provider session: "Replace the conversation with a summary." and **Compact…**, or **Compact after this turn** while the agent is working. Codex: the button disabled with "Not available for Codex agents: the Codex CLI has no compact command." No provider session: disabled with "Available after this agent's first turn."
- **Form:** opened in place by the button; view state only, kept per session like a draft. "What to keep · optional", the consequence line, **Cancel**, and **Compact now** or **Compact after this turn**. While working, the consequence line also says it runs when the turn ends, before waiting messages.
- **Queued:** the form's place shows "Compacts when this turn ends", "Keeping: …" when a note was given, and **Cancel**. When the agent is not in a turn and the request is waiting (decision 3's table), the text is "Compacts when this agent runs again".
- **Running:** "Compacting…", a striped bar from `ui::progress_bar`, and "Messages you send now are delivered when it finishes." No Cancel. The refresh button and the header's Pause button are disabled.
- **Notices** from `last_compaction`, each with **Dismiss**: "Nothing to compact" in the subtle color with "Claude said: <reason> The context is unchanged."; "Compaction failed" in red with the reason and "The context is unchanged."; "Compaction interrupted" in red with "Wiffletree stopped while Claude was compacting. The figure above was measured again.", with no banner in the conversation and no change to the badge A compacted outcome shows no notice; the lower figure is the result. A notice is also cleared by the next request.
- **Chip** (`context_chip`): after the percentage, "compact queued" or "compacting".
- **Badge** (`ui.rs::session_badge`): "Compacting" with the working glyph and color while `compaction.state` is `running`. The sidebar is unchanged.
- **Measuring again:** every outcome triggers one `/context` measurement, when `last_compaction.at` is newer than what the card shows. Unlike a finished turn's measurement, this one also runs while the agent is working. When a message was waiting, the scheduler starts the next turn in the same pass that settles the compaction, so the desktop first sees the outcome with the agent already working; waiting for that turn to end would leave the chip and card at the old figure, against the approved Done state. The probe is read-only (`--no-session-persistence`, no model call). It is the only measurement made during a turn, the refresh button stays disabled then as today, and it never runs while a compaction is running. When an earlier probe is still running as the outcome arrives, the freshness check runs again as that probe returns, since no later snapshot may come.
  *Alternatives:* hold the next turn until the desktop has measured, rejected because the host would wait on a client that may not be open; accept the old figure until the turn ends, rejected because the card would show 81% beside "Context compacted".

### 10. Restart
A `queued` request is untouched by a restart.

A compaction that was running when the host stopped is **interrupted**. It uses the machinery `stop-leftover-provider-processes` built for turns and adds no rule of its own; its run row records the process group and the leader's start time like any run's.

- **Quit.** Decision 4: the process group is stopped before the run is settled, so the host knows it is gone.
- **Startup.** `recover_unfinished_turns` handles an unfinished compaction run in the same loop as an unfinished turn. `stop_leftover` signals the group only while its leader has the recorded pid and start time; the run is marked `interrupted` with `process_stopped_at` and `process_gone`; a group it could not confirm gets the same leftover-process inbox note. The one difference is the session. A turn's session becomes `Disconnected` with its input held. A compaction's is settled by `Host::compaction_ended` with the result `interrupted`, and is otherwise left alone. Each run's settlement is one store unit and writes its line at most once, so a host that dies during recovery finds the run as it was and recovers it again.
- **`compaction_ended`** settles the session's side of every compaction, and both places call it with `interrupted`: the request is cleared, `last_compaction` is `interrupted`, and the `Compaction interrupted` line is written. The host never runs it again. It cannot know whether Claude finished rewriting the session, so it says neither "compacted" nor "context unchanged" and lets the measurement show the truth.
- **The session keeps its status,** such as Ready, Blocked or Paused, and its `last_error`. A compaction delivers no input, so nothing is held and nothing needs a Resume. Waiting messages stay `queued`, and the first turn the scheduler starts takes them.
- **A leftover the host could not stop.** The open note holds the session through `hold_reason`, unchanged: a due turn is held (Blocked, the reason naming the process, the parent told once), the group is checked again every two seconds, and the turn starts when the group exits or the human dismisses the note. The compaction pass asks the same `hold_reason` (decision 3), so a new request waits too.

So no turn and no compaction can start beside a live `/compact` process: a finished run has no live group except the ones startup noted, and every path that starts a provider process for a session asks `hold_reason` first. The Disconnected-until-Resume rule, its `COMPACTION_CUT_OFF` reason and the banner's Resume wording, which an earlier draft of this design used because the host then neither waited for nor identified a provider process, are gone.

One window stays, and it is the earlier change's own, not a compaction's: a host that dies between spawning a provider and recording its group leaves a process no run names. A compaction inherits it and adds no rule for it.

Checked against the code on main e62b305:

- **Waiting input is never cancelled.** This change never calls `reconcile_session`, which cancels a session's queued messages when called as a skip while `runtime.held` is set (`live.rs`). An interrupted compaction changes no receipt and leaves `runtime.held` as it was, so that path cannot be reached by it. A session held beside a leftover process is an ordinary held turn; Skip there is the person's own choice to drop the input, as it is today.
- **What the next turn is told: nothing.** A compaction that succeeds tells the agent nothing either; whatever Claude left in the session is its context. `IS_TURN_RUN` goes on both the outer query and the `MAX(started_at)` subquery of `resume_note`, so the compaction's `interrupted` row neither produces an "Interrupted turn" line nor hides one for a real turn. `interrupted_turn_note` matches runs by their input messages, and a compaction has none.
- **`Host::recover`** marks `Working` sessions `Disconnected` and `delivered` messages `held` when the store opens. A compaction starts only when no turn runs and delivers nothing, so its session is not `Working` and has no `delivered` message, and `recover` leaves it alone.
- **The note's words.** The leftover-process note and hold reason say the process is "from its interrupted turn" and "from an earlier turn". They are left as they are for a compaction's process: the case needs a crash and a process the host cannot confirm, and the instruction they give is the same.

### Decided by Michael on October 13, 2026
1. **A queued compaction stays queued.** When its turn fails, is stopped or paused, is interrupted by a restart, or the project is stopped, the request stays queued, visible and cancellable, and runs when the agent next runs, before its input (decision 3).
2. **The second waiting text** is "Compacts when this agent runs again", shown when the agent is not in a turn (decision 9).
3. **Pause or Stop while a compaction runs lets it finish.** The agent is paused afterwards; a parent's `stop_agents` and a project Stop behave the same way (decision 4).
4. **A compaction cut off by a host restart is never run again.** It leaves the line "Compaction interrupted" and a notice saying the figure was measured again (decisions 8 and 10). The agent keeps the status it had and nothing needs a Resume: the host stops the compaction's process at the quit or at the next start, and holds the agent's turns beside a process it cannot confirm, as it does after an interrupted turn (reconciled with `stop-leftover-provider-processes` with the coordinator, October 13, 2026).
5. **The line prints Claude's own figures,** `pre_tokens` and `post_tokens`, as they are (decision 8).

### Files and functions to change
| File | Functions / items |
|---|---|
| `crates/workspace-core/src/lib.rs`, `usage.rs` | `Compaction`, `CompactionState`, `CompactionOutcome`, `CompactionResult` (new); `SessionRuntime` (`compaction`, `last_compaction`); `Command` (`CompactContext`, `CancelCompaction`, `DismissCompaction`); `COMPACTION_LINE_PREFIX`; `compact_tokens` token formatter (moved) |
| `crates/workspace-host/src/compaction.rs` (new), `lib.rs` | `request_compaction`, `cancel_compaction`, `dismiss_compaction`, `compaction_ended`, the outcome mapping and the line's text; `execute` (three commands); `IS_TURN_RUN` |
| `crates/workspace-host/src/service.rs` | `schedule` (compaction pass first, skip in the message pass), `admit_round` and `start_turn` (request guard), new `due_compactions`, `start_compaction`, `compaction_event` and `compaction_finished`, `stop` (cancel compactions and wait for them with the turns), `IS_TURN_RUN` in `check_in_by_turns`, `stopped_by_parent_note` and `resume_note`, `recover_unfinished_turns` (an unfinished compaction run goes through the same leftover handling, then `compaction_ended`) |
| `crates/workspace-host/src/steps.rs`, `usage.rs` | `IS_TURN_RUN` in the latest-run and unreported-run queries |
| `crates/workspace-host/src/provider.rs` | `Turn` (kind), `input`, `configure`, `run` (`status`, `compact_boundary`, `modelUsage`), `ProviderEvent::Compaction` and `ProviderEvent::ModelUsage` |
| `crates/workspace-host/src/usage.rs` | `record_model_usage`: each result record's `modelUsage` kept as the session's reading, and a compaction recorded as what it rose by |
| `crates/workspace-desktop/src/` | `context_view.rs` (`context_card`, `context_chip`, `sync_context`, form state), `ui.rs::session_badge`, `conversation.rs` (compaction line, Pause), `bridge.rs` |
| Tests | new `crates/workspace-host/tests/compaction.rs`; recorded streams in `tests/fixtures/streams`; unit tests in `provider.rs`, `usage.rs`, `context_view.rs`, `conversation.rs` |
| Docs | `docs/PRD.md` (context paragraph), `docs/DEVELOPMENT.md` (turn scheduling and usage paragraphs) |

## Risks / Trade-offs

- [The summary request may miss the prompt cache] → Decision 5 keeps a turn's arguments so the request has the same prefix, and task 2.1 measures `cacheReadInputTokens` before the argument list is final. The form already says a compaction costs a model call.
- [The CLI's records are not a documented contract] → Only three fields are read (`compact_result`, `compact_error`, the boundary's token counts). An unrecognized stream ends as "failed" with the CLI's own text, and the measurement that follows shows the real context.
- [A request waits a long time on a paused agent] → It is on the chip and the card the whole time and can be cancelled.
- [The line's figures and the card's can differ] → Accepted; decision 8 says why.
- [A compaction on a very full session can itself fail for size] → It ends as "failed" with Claude's reason; Claude's automatic compaction is unchanged and still applies.
- [`last_compaction.at` drives a measurement on every outcome, possibly during the next turn] → One `/context` call with no model request and no session write. Measuring during a turn is new, so task 4.3 checked it on a throwaway session: during a running turn the probe returned a figure in 1.6 seconds and wrote nothing to the session's record, and the turn finished normally and was resumed afterwards.

## Migration Plan

No data migration. The new `SessionRuntime` fields default when absent, and `provider_runs.detail.kind` is absent on every existing row, which is how a turn is recognized. Rollback: an older build ignores the new runtime fields; it draws a `compaction:` message without the one-line treatment, and a compaction run would count as one turn toward a check-in.
