## Context

See proposal.md for why. `compact-context` (archived, PR #49) built compaction for Claude; its design is the base of this one and is not repeated here. What it built that does not depend on the provider, on main 513bfae:

- `workspace-host/src/compaction.rs`: `request_compaction`, `cancel_compaction`, `dismiss_compaction`, `compaction_ended`, the outcome mapping `outcome` and the line's text `line`. `request_compaction` refuses a Codex session.
- `workspace-host/src/service.rs`: the compaction pass in `schedule`, the guards in the message pass, `admit_round` and `start_turn`, `start_compaction`, the `compacting` map, `compaction_event`, `compaction_finished`, and the quit and startup handling from `stop-leftover-provider-processes` (PR #46).
- `workspace-host/src/provider.rs`: `TurnKind::Compaction`, which `input` and `configure` turn into Claude's bare `/compact`; `run` emits `ProviderEvent::Compaction` with a `CompactionRecord::Status` or `Boundary`. `run` writes the input once, then reads one JSON value per line from stdout until the process exits.
- `workspace-host/src/context.rs`: the Codex measurement reads the thread's session record: the last `token_count` entry gives the total and the window, and item sizes give the estimated categories. A `compacted` entry resets the conversation categories.
- `workspace-desktop/src/context_view.rs`: `compact_action`, `compact_form_text`, `compaction_notice` and the form.

### Observed on throwaway threads

codex-cli 0.160.0, `gpt-6.1-sol`, October 13, 2026. Three threads made with two `codex exec` turns each in a scratch directory, then driven over `codex app-server`'s stdio. No agent's thread was used. The exchanges are kept in this change's `recordings/`, one JSON object per line with who sent it, beside three samples of session-record entries (`session-record-*.jsonl`, with the summary's encrypted content cut short).

**The exchange.** The client writes one JSON object per line: `initialize` with `clientInfo`; the `initialized` notification; `thread/resume`; `thread/compact/start` with `{"threadId": …}`. `initialize` was answered in 0.3 seconds and `thread/resume` in 0.8. `thread/compact/start` is answered at once with `{"result":{}}`; the compaction follows as notifications. All four lines can be written before any reply is read: three runs that did so were answered in order and compacted (`all-written-at-once.jsonl`).

**A success** (`success.jsonl`), after the reply to `thread/compact/start`:

1. `thread/status/changed`, `status.type` `active`
2. `turn/started`, `turn.status` `inProgress`
3. `item/started`, `item.type` `contextCompaction`
4. `thread/tokenUsage/updated`: `tokenUsage.last.totalTokens` is the new context total; `tokenUsage.total` is unchanged
5. `item/completed`, the same item
6. `thread/status/changed`, `status.type` `idle`
7. `turn/completed`, `turn.status` `completed`, `turn.error` null, `turn.durationMs`

Between them come notifications that say nothing about the compaction: `mcpServer/startupStatus/updated`, `account/updated`, `remoteControl/status/changed`, `thread/goal/cleared`, `warning`. Eight compactions completed, in 9.6 to 16.8 seconds, on requests of 14k to 30k tokens. The server keeps running afterwards; it does not exit on its own.

**A failure** (`failed.jsonl`), made by resuming with a model that does not exist: steps 1 to 3 as above, then `thread/status/changed` with `status.type` `systemError`; an `error` notification with `error.message` and `willRetry` false; and `turn/completed` with `turn.status` `failed` and the same text in `turn.error.message`. The session record gained no `compacted` entry.

**A second compaction started while one runs** (`second-start-interrupts-first.jsonl`): both requests are answered `{"result":{}}`. The first compaction's turn ends with `turn/completed`, `turn.status` `interrupted`, `turn.error` null, after 0.1 seconds, and the second runs to `completed`. Codex does not refuse a busy thread; it replaces the running compaction.

**Refused requests** (`unknown-thread.jsonl`, `refused-requests.jsonl`), each a JSON-RPC error reply with code -32600:

| Request | `error.message` |
|---|---|
| `thread/resume` with an id Codex has no record for | `no rollout found for thread id …` |
| `thread/compact/start` for a thread that was not resumed, or does not exist | `thread not found: …` |
| `thread/compact/start` with a malformed id | `invalid thread id: …` |
| a method the server does not have | ``Invalid request: unknown variant `…`, expected one of `initialize`, …`` |
| a request with a missing field | ``Invalid request: missing field `threadId` `` |
| any request before `initialize` | `Not initialized` |

**The process stopped mid-compaction.** SIGTERM to the process group three seconds into a compaction: the server exited with code 0 in 0.1 seconds. The session record gained no `compacted` entry, but Codex wrote a developer message beginning `<turn_aborted> The previous turn was interrupted on purpose` and a `turn_aborted` entry. `codex exec … resume` on that thread then ran normally and recalled a codeword from before every compaction.

**What is written to the session record.**

- `thread/resume` writes one `thread_settings_applied` entry holding the model, effort, approval policy and sandbox it was resumed with. Resumed without `sandbox`, Codex wrote a restricted sandbox into it; resumed with a wrong model, it wrote that model. Resumed with no model, effort or instructions at all, the thread kept the ones its turns had used.
- A compaction writes `task_started`, a `token_usage_record`, `compacted`, `thread_settings_applied`, a `token_count`, `item_completed` and `task_complete`. It writes no `turn_context` and no message.
- The `compacted` entry's `message` is empty. What the compaction kept is in its `replacement_history`: the earlier user messages and one item of type `compaction` with `encrypted_content`.

**Figures.** The `token_count` after a compaction has `last_token_usage.total_tokens` equal to the notification's `last.totalTokens` and the same `total_token_usage` as before it. On one thread: 29,532 before, 6,550 after. It leaves out Codex's fixed system prompt and tools: on another thread it read 6,822 after a compaction and 19,882 after the next turn. `thread/resume` sent a `thread/tokenUsage/updated` with the last turn's figure on the three threads whose last entry was a turn, and none on a thread whose last entry was a compaction.

**Tokens** (`session-record-compacted.jsonl`). No notification carries a compaction's own usage. The cumulative usage that `codex exec` reports at `turn.completed`, which is the session record's `total_token_usage`, does not include it: a thread at 124,978 was compacted and reported 144,860 after a turn of 19,882. The compaction's request is only in the session record's `token_usage_record` entry (for example input 29,552, of which 29,312 cached, and output 440), whose own `thread_token_usage` does count it. That entry carries the `turn_id` of the compaction's turn, the id `turn/started` and `turn/completed` name. Each of the eight completed compactions wrote exactly one; the failed, the interrupted and the stopped compaction wrote none (`session-record-failed.jsonl`, `session-record-interrupted.jsonl`). The `compacted` entry repeats it as `latest_token_usage_record`.

**Configuration and the prompt cache.** Started with a turn's configuration, the compaction's request read 29,312 of 29,552 input tokens from the cache, and 26,624 of 27,305 on a second thread. Started with `-c 'mcp_servers={}' -c 'features.apps=false'`, it read 13,056 of 22,799. The tool list is part of what the request shares with the thread's turns.

**Tool servers.** `thread/resume` starts the tool servers of the user's Codex configuration (`node_repl`, `cua_repl`, `codex_apps` here). Each ran in a process group of its own, not the server's. All had exited half a second after the server ended, whether by SIGTERM, by SIGKILL or because its input was closed. The server exits within 0.02 seconds when its input closes.

## Goals / Non-Goals

**Goals:** the Compact action on a Codex agent, behaving as it does on a Claude agent wherever the provider does not force a difference; an outcome read from what Codex reported; a compaction that cannot hang; the compaction's tokens in the usage ledger, counted once.

**Non-Goals:** instructions for a Codex compaction; changing or recording Codex's automatic compaction; running Codex turns through the app-server; a coordinator or agent tool; correcting Codex's figures; any change for Claude agents.

## Decisions

### 1. Everything provider-neutral is reused as built
The request on the runtime, the three commands, the compaction pass and its guards, `start_compaction`, the `compacting` map, `compaction_finished`, `compaction_ended`, the line, the quit and startup handling, the card's queued and running states, the chip and the badge are unchanged. `workspace-core` gains nothing. A Codex compaction is a `provider_runs` row with `detail.kind` `"compaction"` like Claude's, so every `IS_TURN_RUN` query already leaves it out.

The Codex-specific part is: the request rule (decision 2), one way to run the provider (decisions 3 to 5), one way to read its outcome and figures (decisions 6 and 7), the ledger (decision 8) and wording (decision 10).

### 2. A Codex request carries no instructions
`request_compaction` no longer refuses a Codex session. It refuses a Codex request that carries instructions, with "Codex chooses what the summary keeps; it takes no note." `thread/compact/start` has no field for them, and dropping a note silently would show a compaction that kept nothing the person asked for. The desktop shows no field for a Codex agent and sends none, so a person never meets the refusal.

The other refusals are unchanged: no provider session, archived, a request already waiting.

### 3. One `codex app-server` process per compaction
`provider::configure` gains a Codex compaction case: `codex app-server` in place of `exec`, with the same `-c` overrides as a turn (`model_reasoning_effort`, `developer_instructions`, and the `agent_workspace` tool server with a fresh token that is never registered, as for a Claude compaction), in the agent's directory, leading its own process group like every provider run. `exec`'s own flags (`--json`, `--dangerously-bypass-approvals-and-sandbox`, `-m`, `resume`) are not app-server flags; their equivalents go in `thread/resume`.

`provider::input` returns the four lines of the exchange, written at once:

1. `initialize` with `clientInfo` naming Wiffletree
2. the `initialized` notification
3. `thread/resume` with `threadId`, `cwd`, `model`, `sandbox` `"danger-full-access"`, `approvalPolicy` `"never"` and `excludeTurns` true
4. `thread/compact/start` with `threadId`

`run` keeps the server's input open until the outcome is read, because the server exits when it closes. That is the only change to how `run` drives a process: it still writes once and reads lines. `Spawned` is emitted as for any run, so the run row records the group and its leader's start time.

*Why a turn's configuration.* `thread/resume` writes the settings it was given into the thread's record, so they must be the turn's own: the model the agent's profile names and the same sandbox and approval policy. The observed cache figures say the tool list is part of the request the compaction shares with the turns, so the app-server gets the turn's tool servers, `agent_workspace` included. Decision 5 of `compact-context` reached the same answer for Claude for the same reason. The developer instructions are already in the thread's record from its first turn; passing the same text again changes nothing and keeps one argument list.

*Alternative:* wait for each reply before writing the next request. Rejected for now because `run` would need a second way to drive a process, and three runs showed the server answers in order. If a later Codex answers out of order, `thread/compact/start` is refused with "thread not found", which decision 6 turns into a failed compaction with that reason: visible, and never a hang. Task 2.1 repeats the check with a real agent's configuration before the code is written.

*Alternative:* Codex's long-lived app-server daemon, shared across compactions. Rejected: a process the host does not own cannot be stopped at a quit or confirmed at startup.

### 4. Stopped like any provider run
- **When the outcome is read,** `run` closes the server's input and stops the process group the way it ends every run (`Process::stop`). The server was seen to exit on its own within 0.02 seconds of its input closing; the stop does not depend on that.
- **Quit and startup** are `compact-context` decision 10 without a change. The cancel flag makes `run` stop and reap the group before it reports the end, and the compaction is settled as interrupted. At startup an unfinished compaction run goes through `stop_leftover` and the leftover-process note like any run.
- **Tool servers** run in process groups of their own, so stopping the server's group does not signal them. They were seen to exit when the server did, however it ended. A `codex exec` turn starts the same servers and the host relies on the same behaviour for turns today. This change adds no rule for them. A tool server that outlived the server would be a leftover no run names, the same gap `compact-context` decision 10 already records for a host that dies between spawn and record.

### 5. What Codex writes when a compaction is cut off
A compaction stopped by SIGTERM leaves Codex's own developer message in the thread, "The previous turn was interrupted on purpose…". The agent's next turn reads it. The host adds nothing of its own, as for Claude: its prompt names no interrupted turn. The message is Codex's and is left as it is.

### 6. The outcome is read from the turn's completion
`run` gains one arm, for a Codex compaction, that reads each line as a JSON-RPC message and emits the `CompactionRecord`s the host already settles with, and the compaction turn's id for decision 8:

1. A reply with `error` to any of the three requests → `Status(Err)`, reason `Codex refused ‹method›: ‹error.message›`. The first one decides; a refused `thread/resume` is followed by a refused `thread/compact/start` and only the first is kept.
2. `turn/completed` with `turn.status` `completed` → `Status(Ok)`.
3. `turn/completed` with `turn.status` `failed` → `Status(Err)` with `turn.error.message`.
4. `turn/completed` with any other status, or none that can be read → `Status(Err)`, reason `Codex ended the compaction as ‹status›`. This covers `interrupted` and a shape this version does not know.
5. A request from the server (a message with both `id` and `method`) is answered with a JSON-RPC error, so the server is not left waiting. No such request was seen: the thread is resumed with approval policy `never`.
6. Every other notification is ignored, the `error` notification included: the `turn/completed` that follows it carries the same text.

`compaction::outcome` then maps the status as it does for Claude, with no change: `Ok` is **compacted**, `Err` is **failed** with the reason, and no status at all is **failed** with what the run ended with. "Not enough messages to compact" is Claude's text and Codex never sends it, so **nothing to compact** does not arise. The one edit in `outcome` is the fallback reason, which names the agent's provider instead of always Claude.

Two endings have no status:

- **The process ends first,** or cannot start, such as a Codex without the `app-server` command. `run`'s existing ending reports "Provider exited without a successful terminal result" with the exit status and the end of stderr.
- **Nothing terminal arrives.** A Codex compaction has a limit, `CODEX_COMPACTION_LIMIT`, counted from the process's start. When it passes with no outcome read, `run` stops the group and ends with "Codex did not finish compacting within 10 minutes." Compactions took 10 to 17 seconds on small contexts; the limit is a backstop, not a budget. Claude's compaction needs none, because the Claude CLI exits by itself.

Success needs `turn.status` to be exactly `completed`. A changed protocol therefore errs toward "Compaction failed" with a reason, and the measurement that follows every outcome shows what the context really is.

**A failed compaction says "context unchanged",** as for Claude. For the failure that was observed it is true: the record gained no `compacted` entry. For the limit and for a process that died, the host cannot know, as it cannot for Claude; the card is measured again either way.

### 7. The figures
`ProviderEvent::Compaction(CompactionRecord::Boundary { before, after })` carries them, as for Claude:

- **Before** is the context total the card shows: the last `token_count` in the thread's session record, read by `context.rs` as the compaction starts. `thread/resume` does not always send a figure of its own (it sent none on a thread that had just been compacted), so the record is the one dependable source.
- **After** is `tokenUsage.last.totalTokens` of the `thread/tokenUsage/updated` that arrives after `turn/started`.

When either is missing the line reads "Context compacted" with no figures, as today.

Both are Codex's figures for the whole context, printed as they are. **They read low right after a compaction:** the figure Codex reports then leaves out its fixed system prompt and tools, about 13k tokens on the threads observed, and the card's measurement reads the same entry. The agent's next turn reports the full total again. Michael was told this on October 13, 2026 and decided the figures are not corrected. For Claude the second figure is the summary alone; `compact-context` decision 8 accepted the same kind of difference.

### 8. One ledger row, read from the session record
A Codex compaction that ends as compacted is recorded in the usage ledger as one request. Michael decided this on October 13, 2026; the first draft of this design recorded nothing.

**Where the tokens are.** No notification reports them, and the cumulative usage `codex exec` reports does not include them. They are in the thread's session record, in the `token_usage_record` entry Codex writes for the compaction's one model request. This is an entry of Codex's own file that no interface documents. It is the same file `context.rs` already reads for the card, and it is read the same way: by entry type, taking nothing for granted.

**Which entry.** Every `token_usage_record` carries the `turn_id` of the turn that made the request, and `turn/completed` carries that turn's `turn.id`. On the three throwaway threads, each of the eight completed compactions had exactly one `token_usage_record` with its turn's id; the failed one, the one a second request interrupted and the one stopped by SIGTERM had none; and no turn's entry carried a compaction's id. So the entry is identified by id, not by position:

- The Codex arm of `run` (decision 6) also emits the compaction turn's id from `turn/completed`, as a third `CompactionRecord`.
- When the compaction settles as compacted, the host reads the session record and takes the `token_usage_record` entries whose `turn_id` is that id.
- **Exactly one,** whose `usage` has counts `usage::normalize` accepts → one ledger request.
- **None, more than one, an unreadable `usage`, no turn id, or no session record** → no request. The compaction still settles as compacted, and the run's `detail.result` says why nothing was recorded. The host never falls back to another entry, to the newest entry, or to a difference of totals.

Only the entry's `usage` is read. Its `turn_token_usage` and `thread_token_usage` are running sums and are never used.

**When it is read.** In `compaction_finished`, after the provider thread has reported the end. By then `run` has stopped the app-server's process group, so the server is no longer writing the file. On the one compaction whose times were compared, the entry was written 18 milliseconds before `turn/completed` was sent.

**The row.** `usage` uses the names `normalize` already reads for Codex, with Codex's convention that the input count includes cached tokens:

| `token_usage_record.usage` | Ledger column |
|---|---|
| `input_tokens` | input |
| `cached_input_tokens` | cache read |
| `cache_write_input_tokens` | cache write |
| `output_tokens` | output |
| `reasoning_output_tokens` | reasoning |

The request id is `compact:<run>`. A turn's is `turn:<run>` and Claude's compaction rows are `compact:<run>:<model>` under the other provider, so it cannot collide. The model is the run's profile model, the time is the run's start as for every Codex row, `complete` is true, and the entry's `usage` is kept as the row's native record. It is attributed to the agent and its coordinator like any request.

**It is not a counter reading.** Every other Codex row is what the thread's cumulative usage rose by (`usage.rs::codex_delta`). This row is one request's own usage, so it is written by a function of its own that does not go through `codex_delta`: it neither reads nor moves `usage_counters`. The cumulative total does not include the compaction, so the next turn's row holds exactly that turn's tokens. Nothing is counted twice and nothing has to be subtracted.

**The continuity check.** `codex_delta` marks a turn's row incomplete unless the session's run just before it took the counter's last reading. A compaction run sits between two turns and takes no reading, so the turn after it would be recorded as partial. The "run just before" lookup therefore skips compaction runs with `IS_TURN_RUN`.

**Written once.** The row is inserted in the same store unit in which `compaction_finished` settles the run and `compaction_ended` clears the request. A host that dies before that unit leaves the run unfinished; the next start settles it as interrupted and records nothing, and the card says "Compaction interrupted", so the missing row is never silent. A host that dies after it finds the run finished and never settles it again. The request id is keyed by the run as well, so the ledger's unique key refuses a second row.

**Failed and interrupted compactions record nothing.** Codex wrote no entry for the ones observed, and the host does not look.

The Usage page gains nothing: the request counts in its existing totals, as Claude's does.

### 9. The context estimate after a compaction
`context.rs` already resets the conversation categories at a `compacted` entry, but it sizes the summary from the entry's `message`, which Codex leaves empty. It now sizes what the entry's `replacement_history` holds, through the same `codex_item` as any other item: the kept user messages count as Messages, and the `compaction` item's `encrypted_content` counts as Messages too, sized the way an encrypted reasoning item is. Without this the card shows no Messages at all after a compaction, against the approved Done frame. The total is untouched: it is Codex's own.

### 10. Desktop
All of it matches state 7 of `design/mockups/current/compact-context.html`.

- **Row** (`compact_action`): the provider no longer matters. With a provider session: "Replace the conversation with a summary." and **Compact…**, or **Compact after this turn** while working. Without one: disabled beside "Available after this agent's first turn."
- **Form:** for a Codex agent there is no "What to keep" label and no field, and the idle consequence line reads "Codex replaces this agent's conversation with a summary and chooses what the summary keeps. This can't be undone and uses one model call." The working line is the one Claude's form shows; it names no provider. `confirm_compact` sends no instructions for a Codex agent, whatever an earlier draft for that session holds.
- **Queued and running:** unchanged. A Codex request has no instructions, so no "Keeping:" line appears.
- **Notices** (`compaction_notice`): the interrupted notice names the agent's provider, "Wiffletree stopped while Codex was compacting. The figure above was measured again." The failed notice shows the reason of decision 6. The "Nothing to compact" notice is unchanged and is never produced for Codex.
- **Line, chip, badge, measuring again:** unchanged.

### Decided by Michael on October 13, 2026
1. **A Codex compaction is recorded in the usage ledger,** read from the session record's `token_usage_record` entry for that compaction's turn. When the record does not hold exactly one such entry, no row is written and the compaction is still reported as compacted (decision 8).
2. **A Codex compaction may take 10 minutes** before the host stops it and reports it failed (decision 6). Nothing longer than 17 seconds was seen, on contexts of at most 30k tokens; a full window was not tried.
3. **A Codex request that carries instructions is refused,** not accepted with the note dropped (decision 2).
4. **Codex's own "interrupted on purpose" message stays in the thread** after a compaction the host cut off (decision 5).
5. **The figures are printed as Codex gives them,** though they read low until the agent's next turn (decision 7).

### Files and functions to change
| File | Functions / items |
|---|---|
| `crates/workspace-host/src/compaction.rs` | `request_compaction` (accept Codex, refuse its instructions); `outcome` (the fallback reason names the provider) |
| `crates/workspace-host/src/provider.rs` | `input` (the four lines), `configure` (`app-server` and a turn's `-c` overrides), `run` (keep the input open, the Codex compaction arm of decision 6, `CODEX_COMPACTION_LIMIT`) |
| `crates/workspace-host/src/service.rs` | `start_compaction` (read the context total for a Codex agent and hand it to the run); `compaction_event` (keep the turn's id); `compaction_finished` (the ledger row, in the settling unit) |
| `crates/workspace-host/src/usage.rs` | `codex_delta` (`IS_TURN_RUN` in the preceding-run lookup); a new `record_codex_compaction` that writes one request without touching `usage_counters` |
| `crates/workspace-host/src/context.rs` | `codex` (size a `compacted` entry's `replacement_history`), `codex_item` (the `compaction` item); a reader for the `token_usage_record` entries of one turn |
| `crates/workspace-desktop/src/context_view.rs` | `compact_action`, `compact_form_text`, `compaction_notice`, the form in `compact_section`, `confirm_compact` |
| Tests | `crates/workspace-host/tests/compaction.rs`; a `codex app-server` branch in `tests/fixtures/provider.py`; recorded exchanges in `tests/fixtures/streams`; unit tests in `provider.rs`, `compaction.rs`, `usage.rs`, `context.rs`, `context_view.rs` |
| Docs | `docs/PRD.md` (context paragraph), `docs/DEVELOPMENT.md` (compaction and usage paragraphs) |

## Risks / Trade-offs

- [The app-server protocol is experimental and may change] → The host reads four things: a reply's `error.message`, `turn/completed`'s `turn.status` and `turn.error.message`, and `tokenUsage.last.totalTokens`. A missing method is a refused request, a changed status is "ended as ‹status›", a missing notification is the limit. Each ends as "Compaction failed" with a reason on the card and a line in the conversation; the test "A Codex without the compact method" pins the first and "No completion arrives" the last.
- [Requests are written before their replies are read] → Decision 3; an out-of-order server is a failed compaction with Codex's reason. Task 2.1 checks it again.
- [The figures read low until the next turn] → Decision 7; decided, and stated in the mockup's caption.
- [The ledger row depends on an entry of Codex's session record that no interface documents] → Decision 8. The entry is matched by the turn's id and only when exactly one matches; anything else writes no row and the compaction is still reported as compacted. The test "No usage entry in the record" pins it. A Codex that stops writing the entry costs the row, never a wrong one.
- [Tool servers are not in the process group the host stops] → Decision 4; the same as for turns today, and they were seen to exit with the server.
- [`thread/resume` writes settings into the thread's record] → They are the turn's own settings (decision 3). The test "Resumed as a turn would be" pins the request's fields.
- [A second client compacting the same thread interrupts the first] → The host starts at most one compaction per session and none beside a turn; `hold_reason` keeps both away from a leftover process.
- [The user's Codex configuration changes what `thread/resume` starts] → It already decides what a turn starts; a compaction uses the same.

## Migration Plan

No data migration: no new field, table or command. Rollback: an older build refuses a Codex request again and shows the row disabled; a Codex compaction's line and run row are the ones it already draws and skips for Claude, and its ledger row is an ordinary request.
