## Why

Reordering projects by dragging landed with four gaps in the sidebar's drag handling, recorded as follow-ups: a row could not be dragged to a place that was off-screen, a drag brought back to its own row acted as a click, a click that moved a few pixels on a project's chevron or as the second click of a double-click did the wrong thing, and a right or middle press that ended a rename left the next drag dead. A host test of stopping and resuming a child also failed under load.

## What Changes

- **The list scrolls under a dragged row.** While a project or group is dragged with the pointer near the top or bottom edge of the list, the list scrolls that way, faster the nearer the edge and also while the pointer rests, until it reaches the end. The insertion line follows the rows as they move.
- **A drag brought back to its own row is cancelled.** A press and release that never left the row is still the click it would have been without moving.
- **A moving click does what a still click does.** On a project's chevron it collapses or expands the project, and as the second click of a double-click it opens rename.
- **The press that ends a rename is forgotten on its release**, whichever button it was and wherever it is released, so the next drag works.
- **The stop and resume host test reads the latest logged prompt.** The stopped turn's provider logs its prompt only once it is running, and under load the stop arrived first, so the resumed turn's prompt was the first block of the log, not the second. The test's fixed half-second wait is gone: it had been covering for the test resuming a stopped child before that child's turn had finished, which the host refuses, so the test now waits for each stopped turn to finish. The fake provider replaces its usage file whole, so a turn stopped mid-write no longer leaves it empty for the next turn. The host is unchanged.
- **Out of scope.** Escape to cancel a drag, keyboard or menu moves, and any change to grouping rules, the host's order command or the store schema.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `native-workspace`: reordering by dragging gains scrolling at the list's edges and the rule for a drag released on its own row.

## Impact

- `crates/workspace-desktop/src/tree.rs`: `edge_scroll` and `own_release`, the two decisions, with unit tests.
- `crates/workspace-desktop/src/sidebar.rs`, `main.rs`: scrolling under a drag, the release on a drag's own row, and the rename press.
- `crates/workspace-desktop/src/automation.rs` and `scripts/drive_desktop.py`: development-only `press` with a click count or the right button, so a moving double-click and a right press can be driven.
- `crates/workspace-host/tests/coordinator_control.rs`.
- `docs/DEVELOPMENT.md`.
