## Why

The Compact action on the Context card works for Claude agents only (`compact-context`, PR #49). On a Codex agent the row is shown disabled, because `codex exec` has no compact command. Codex can compact a thread through its app-server, and Michael decided on October 13, 2026 that Codex agents get the same action. This change adds the one Codex-specific part: a way to run that compaction and read how it ended.

## What Changes

- **Compact is available on a Codex agent's Context card.** The same row, button, queue, running state, notices and conversation line as for Claude. An agent with no provider session still shows the action disabled.
- **No "What to keep" for Codex.** Codex's compact request has no field for instructions, so the form has no field and says that Codex chooses what the summary keeps. The host refuses a Codex request that carries instructions.
- **The host runs `codex app-server` for one compaction.** It resumes the agent's thread with a turn's model and configuration, sends `thread/compact/start`, reads the notifications until the compaction's turn completes, and stops the process. It is one provider run in one process group, so a quit and a restart handle it as they handle any run.
- **The outcome comes from the turn's completion notification.** `completed` is a success. A failed or interrupted turn, a refused request, a process that ends early and a compaction that never completes all end as "Compaction failed" with a reason a person can read. "Nothing to compact" does not arise: Codex compacts whenever it is asked.
- **The line prints Codex's own figures:** the context total before and after. Right after a compaction Codex's figure leaves out its fixed system prompt and tools, so the line and the card read low until the agent's next turn. The figures are printed as Codex gives them.
- **No usage ledger row.** Codex's notifications and its running total report no tokens for a compaction, so the host records none, and the agent's next turn is still counted whole.
- **The protocol is experimental.** A Codex version without the method, or with a changed shape, ends as a failed compaction that names what Codex refused. It never hangs: a compaction that does not complete within a fixed limit is stopped and failed.
- **Not included.** Instructions for a Codex compaction, changing Codex's automatic compaction, moving Codex turns to the app-server, a coordinator or agent tool, and anything for Claude.

## Capabilities

### New Capabilities
<!-- None. Compaction exists; this change adds a second provider to it. -->

### Modified Capabilities
- `workspace-host`: a Codex agent's request is accepted without instructions; a Codex compaction runs through the app-server; its outcome is read from the turn's completion; it adds no ledger row; the Codex context estimate counts what a compaction kept.
- `work-stream`: the compaction line for a Codex agent, with whole-context figures and never a "Kept:" line.
- `native-workspace`: the Compact row enabled for a Codex agent, the form without a field, and notices that name the agent's provider.

## Impact

- `workspace-host`: `compaction.rs` (the request rule, the provider's name in a reason), `provider.rs` (the app-server's arguments and input, one arm that reads its messages, the limit), `usage.rs` (a compaction run does not break the Codex counter's continuity), `context.rs` (what a `compacted` entry kept), `service.rs` (the context total read as a Codex compaction starts).
- `workspace-desktop`: `context_view.rs` (row, form and notice wording by provider).
- `workspace-core`: no new type, field or command.
- Tests: `crates/workspace-host/tests/compaction.rs`, a `codex app-server` branch in `tests/fixtures/provider.py`, and recorded exchanges moved from this change's `recordings/` into `tests/fixtures/streams`.
- Docs: `docs/PRD.md` and `docs/DEVELOPMENT.md`.
- Mockup: `design/mockups/current/compact-context.html`, state 7, approved October 13, 2026.
