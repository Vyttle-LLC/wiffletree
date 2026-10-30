## Why

While an agent works, its conversation shows only a fixed 128-pixel card with the latest narration line and the three latest actions. What the agent says and the diffs of the files it changes are visible only in the turn history sheet, so following a turn means opening it again and again.

## What Changes

- **Live work flows into the conversation**, after the last message and in turn order, as in the study the human approved (`ui/mockups.html`, sections A and B): narration in full with its file references linked; each file edit with its diff open, its first 10 lines and then "N more lines" up to the lines kept; each command as one line with its state and note, or its running time, red when it failed, with its output on click.
- **Quiet steps fold.** Each unbroken run of reads, searches, thinking, plan updates and other tool calls becomes one muted line such as "Read 4 files · searched 2 times · 1 tool call", which opens in place. A failed quiet step breaks out of its run and shows on its own, red.
- **A status line replaces the card**, pinned between the transcript and the composer at a fixed height: spinner, "Working", elapsed time, step count, files changed and the current step. It opens the turn's history, and offers "Jump to latest" when the reader has scrolled up.
- **Collapsible.** The status line's chevron hides the live work and leaves the line, which then shows the latest narration. It is the existing minimize preference, so it applies to every agent and survives relaunch.
- **Scrolling** follows new work only while the reader is at the bottom.
- **Unchanged:** the step fetch and its pacing, finished turns' "Worked … · N steps" line, and the history sheet.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `work-stream`: the live activity card becomes live work in the conversation with a status line; the history sheet and the finished-turn summary are reached from the status line instead of the card.

## Impact

- `workspace-desktop`: `live_work.rs` (new: grouping, folded labels, feed rows); `activity.rs` (status line, live step rows, shared narration linking); `history_sheet.rs` (diff preview limit, shared rendering); `transcript.rs` (live rows in the transcript list, tail following, status line); `settings.rs` (toggle wording).
- Docs: `docs/PRD.md` Live work; the study in `ui/mockups.html`.
