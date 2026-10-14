## Context

`2026-10-13-compact-codex` is the design this builds on; decision 4 there says the host stops the app-server's process group as soon as the outcome is read, and "As built" records that an early exit keeps `run`'s long reason.

## Decisions

### 1. One second for the server to exit by itself
When `run` reads the outcome it closes the server's input at once and notes when the group is to be stopped: one second later (`codex_compaction::EXIT_MARGIN`). The loop stops the group when the server has exited or that time has come, whichever is first. The limit sets that time to now, and a cancellation leaves `run` as before, so neither waits.

*Why one second.* The server was seen to exit within 0.02 seconds of its input closing, and six real compactions wrote the record's last entries before the outcome was reported, so today the margin is never used up. It exists for a Codex that writes those entries later. A second is fifty times the exit that was seen, is the grace a stopped group already gets between SIGTERM and SIGKILL, and is small beside a compaction's ten or more seconds. A shorter bound, such as 0.1 seconds, would rest on one idle machine's timing. A server that ignores its input closing costs the agent one second per compaction and nothing else.

*Alternative:* wait for the exit without a bound. Rejected: a server that never exits would hold the agent until the ten-minute limit.

### 2. The reason is bounded in `compaction::outcome`
`outcome` is where a run's own ending becomes the reason, when no status was read. Bounding it there covers Codex's early exit and the same ending of a Claude compaction with one line of code, and the stored outcome, the event and the card then agree. `bounded` moves from `codex_compaction.rs` to `compaction.rs` for that. A reason Claude gave in its status record is not touched.

The whole text is kept as `detail.result.reported`. A Codex run already had it; a Claude run gains the field, so bounding loses nothing for either. `reported` therefore holds whatever the run ended with: the provider's text for a failure it reported, or the host's words for a quit, the limit or an early exit.

### 3. A failed turn's message must have a first line
The reason is the message's first line. When that line is blank the turn is named by its status, like a failure with no message. What Codex did say, when it said anything, is still kept as `reported`, for any status.

## Risks / Trade-offs

- [The fake app-server now exits when its input closes, as the real one does] → A test that the host stops a server which stays needs the fake's `ignore_input_closing`; one test has it.
