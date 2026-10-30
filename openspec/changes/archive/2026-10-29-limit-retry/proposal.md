## Why

A turn that hits its provider's usage limit (for example Claude's "You've hit your session limit · resets 7pm") fails like any other turn: its input is held, the session is interrupted and nothing happens until the human clicks Retry. The limit resets on its own, so the click is the only thing in the way.

## What Changes

- **A limit failure is recognised.** A turn that fails after a quota reading during the turn said its provider's limit is reached (a window reported rejected or full, with its reset ahead or unknown) is a limit failure. When no reading said so, the provider's error text is matched instead ("usage limit", "session limit", "weekly limit", "usage_limit_exceeded", or "hit your" with "limit"). Claude sends the reading before the failure; Codex readings are not streamed during a turn, so Codex relies on its error text.
- **It is retried at the reset.** The input stays held, as today, and the session records `limit_retry` (when it failed and when the limit resets). The scheduler's pass releases it as Retry does once the limit has reset: at the known reset, or, when the reset is unknown, at the first reading since the failure that no longer says the limit is reached. It reuses the existing reset machinery: a wake at the reset and a pass on every new reading. The same session, provider and model resume; the retried input carries the usual interrupted-turn note.
- **Only the provider's own failure counts.** A turn stopped by the human or its parent, a quit, a PR keeper turn and a session paused at the failure keep today's manual Retry and Skip.
- **Retry and Skip still work.** Retry runs the input now and Skip drops it; either ends the wait. A session paused during the wait waits for the human.
- **The parent is told once.** Instead of the error message asking it to resume the child, the parent gets one message saying the child hit the limit and is retried automatically at the reset, and that nothing is needed. The human's own coordinator retries the same way.
- **It survives a restart.** The wait is stored on the session's runtime and its input stays held, so a restarted host retries it.
- **The conversation says so.** The held-input banner adds "Waiting for Claude's usage limit to reset. Retries automatically today 19:00." under the error, keeps the warning that interrupted tools may already have run, and turns yellow.
- **Contract text:** coordinator core v7 and the `resume_agent` description say a limit failure needs nothing from the coordinator.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `model-routing`: a turn that fails on a usage limit waits for the reset and is retried automatically.

## Impact

- `crates/workspace-core/src/lib.rs`: `SessionRuntime.limit_retry`, `LimitRetry`.
- `crates/workspace-core/src/messages.rs`: `turn_limit_body`.
- `crates/workspace-host/src/service.rs`: recognising a limit failure at turn end; the parent's message.
- `crates/workspace-host/src/service/scheduling.rs`: `retry_after_limits` in the scheduler's pass.
- `crates/workspace-host/src/policies.rs`: `limit_reached`; `any_waiting` counts limit waits.
- `crates/workspace-host/src/live.rs`: Retry and Skip end the wait.
- `crates/workspace-desktop/src/conversation.rs`, `wording.rs`: the banner line.
- `crates/workspace-host/skills/core/coordinator.md` (v7), `mcp.rs`, contract tests.
