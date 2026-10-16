## Why

The decision card above the coordinator's composer covers the conversation the human may need to read before deciding. Today the only ways to get the space back are to answer or to Dismiss, and Dismiss clears the question without waking the coordinator. The human approved a third way: minimize the card to one row and restore it later (mockup: `design/mockups/current/question-card-minimize.html`).

## What Changes

- **Minimize control.** The row above the card gains a ghost icon button at its right end, after Open inbox, tooltip "Minimize".
- **A one-row bar.** Minimized, the card becomes one row with the same yellow edge, a small yellow dot, the current decision's prompt on one line with an ellipsis, "N of M" when there are several decisions, and an Expand button. Clicking anywhere on the row restores the full card on the same decision.
- **Remembered per project, in memory only.** The state is kept for each project. It survives reading, typing, sending and switching conversations, and is lost when the app restarts. A new decision arriving does not re-expand it. When a project has no open decision left, its state is cleared, so the next decision starts expanded.
- **Questions and approvals alike.** Both share the card, so both minimize the same way. Nothing else changes.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `native-workspace`: the decision card above the composer can be minimized and restored.

## Impact

- `crates/workspace-desktop/src/main.rs`: a set of minimized project IDs on the view and the rule that clears it.
- `crates/workspace-desktop/src/conversation.rs`: the Minimize and Expand controls and the minimized row in `decisions()`.
- Tests: the clearing rule in `conversation.rs`.
- Docs: `docs/DEVELOPMENT.md`, `docs/PRD.md`.
- No host, storage or protocol change.
