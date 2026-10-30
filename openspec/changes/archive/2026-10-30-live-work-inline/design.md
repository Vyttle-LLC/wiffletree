## Context

The transcript is one virtualized GPUI list: one row per loaded message, plus one activity row while the agent works. Steps arrive through `StepFetch`, about 30 pages a second at most, each holding only steps changed since the last revision. A run keeps up to 2,000 steps.

## Decisions

- **Live work is rows of the same list.** Each feed row is one list item after the messages: a narration, an edit, a command or failed quiet step, or a folded run. The list measures and draws only what is on screen, so a 2,000-step run mounts nothing eagerly. A row records its step range and the newest revision among its steps; after each page the transcript splices from the first row that differs, so earlier rows keep their measured heights.
- **Grouping is a pure function** (`live_work::feed`) over the run's steps, recomputed per page. It is linear and cheap next to drawing.
- **The status line sits outside the list**, between it and the composer, so it never scrolls away, keeps a fixed height and can offer "Jump to latest".
- **Following uses the list's tail mode** (`FollowMode::Tail`): it follows while the reader is at the bottom, stops when they scroll up and resumes when they return. "Jump to latest" re-engages it.
- **Collapse reuses `minimize_activity`.** Collapsed, the list holds no feed rows at all.
- **Diff preview** clips each file's kept lines to the first 10 across its hunks (`history_sheet::clipped`). While clipped, the toggle counts the kept lines left out and the file's note of lines never kept waits until every kept line is shown. The history sheet draws diffs with no limit, as before.
- **Shared drawing.** `step_row`, `state_glyph`, `detail`, `file_diff` and narration linking (`LinkedNarration`) serve both the history sheet and the conversation.

## Risks

- A folded run that keeps growing is measured again on each page; it is one short line unless opened.
