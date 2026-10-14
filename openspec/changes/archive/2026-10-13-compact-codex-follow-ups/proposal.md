## Why

Compact for Codex agents merged in PR #56 with a list of small follow-ups from its review. Two of them change what the host does and so what the `workspace-host` spec says; the rest are tests, docs and comments that the spec already covers.

## What Changes

- **A margin before the app-server is stopped.** When a Codex compaction's outcome is read, the host closes the server's input and gives it up to one second to exit by itself before stopping its process group. Codex 0.160 writes the last entries of the thread's session record a few milliseconds before it reports the outcome, but no interface promises that order. The limit, a cancellation and a quit still stop it at once.
- **A process that ends early leaves a one-line reason.** The reason of a compaction whose process ended before an outcome was read was stored whole, up to about 1,570 characters over several lines, and only the card cut it. It is now held to one line of 200 characters where it is stored, for a compaction of either provider, because one code path serves both. The run keeps the whole text in `detail.result.reported`, which a Claude compaction's run now has too.
- **A failed turn with no message to show is named by its status,** as the spec already says. An empty `turn.error.message`, or one that starts with a line break, gave an empty reason. A scenario is added; the requirement's words do not change.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `workspace-host`: the bounded time a Codex app-server gets to exit once its outcome is read; the one-line reason of a compaction whose process ended first, for either provider; a scenario for a failed turn with no message to show.

## Impact

- `crates/workspace-host/src/provider.rs`: `run` closes the input when the outcome is read and stops the group when the margin has passed.
- `crates/workspace-host/src/provider/codex_compaction.rs`: `EXIT_MARGIN`; `completed` names a failed turn by its status when its message has no first line.
- `crates/workspace-host/src/compaction.rs`: `bounded` moves here; `outcome` bounds the reason a run ended with.
- `crates/workspace-host/src/service.rs`: `compaction_finished` keeps `reported` for either provider.
- `crates/workspace-host/tests/compaction.rs`, `tests/fixtures/provider.py`: the fake app-server exits when its input closes, and can write its record late, write it for a failed turn, or ignore the close.
- `docs/DEVELOPMENT.md`.
- No data migration.
