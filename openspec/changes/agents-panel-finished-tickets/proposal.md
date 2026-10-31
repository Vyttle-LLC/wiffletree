## Why

The Agents panel draws every accepted ticket as a full card and every unarchived agent as a full row, so a project's finished work buries its open work and idle agents read as busy. MUDling's 32 finished tickets sit among its 9 open ones. Closed tickets are the opposite: they are left out of the board and do not appear at all. The human's rule for agents: "If they are active. eg. waiting on a pr then they are active. If they finished and are done then archive."

## What Changes

- **Finished tickets fold.** Accepted and closed tickets move into one "Finished tickets (N)" section, closed by default, above "Previous agents". Each is one line: its title and its real PR outcome, with no bar, checks or conflict.
- **A finished ticket with an unarchived agent stays a card** on top with that agent, such as one restored from the archive.
- **The panel's heading counts what it draws,** such as "9 open · 32 finished".
- **Waiting agents go quiet.** An idle agent of an open ticket with nothing running sits on one line with no bar or model, saying what it waits for. Working, blocked, paused, disconnected and failed agents keep their full row.
- **A verifier is archived the moment it passes,** not when its round ends, and lists under Previous agents.
- **Not included:** the board, and so the project's progress bar and percentage, the working and stuck counts; automatic acceptance when a PR merges; the PR watcher, which keeps these lines true in its own change.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `work-progress`: the Agents panel folds finished tickets, counts what it draws and quiets waiting agents.
- `agent-lifecycle`: a verifier retires as it passes, not at the end of its round.

## Impact

- `workspace-host`: `verification.rs` archives a verifier when its pass is recorded; the failed round no longer has passes to archive.
- `workspace-desktop`: `agents_panel.rs` (`Roster`, `TicketCard`, the fold and the quiet line), `inspector.rs` (the heading's counts and the fold's state), `wording/tickets.rs`.
- Docs: `docs/PRD.md`, `docs/DEVELOPMENT.md`, `design/README.md`, the study in `ui/mockups.html`.
