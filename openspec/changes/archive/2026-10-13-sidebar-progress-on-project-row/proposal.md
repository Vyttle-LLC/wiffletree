## Why

A thin bar and a figure on every project, ticket and agent row made the sidebar tree busy and every row two lines tall. The human chose to keep progress on the project row only ("Option B" of the sidebar progress mockup). The strip under the conversation title and the Crew panel already show each ticket's and agent's progress.

## What Changes

- **Project row only.** The project row keeps its percentage and thin bar. Ticket rows and agent rows show neither a bar nor a figure ("70%", "blocked", "3 of 5", "working", "done", "passed", "failed").
- **Single-line rows.** Ticket and agent rows return to their height from before the bars, with no track line. The project row keeps its two lines.
- **Unchanged.** Whatever is not progress on those rows: icon, label, PR line, state dot or waived mark, status glyph, "held" and "queued input" notes, and the questions figure. The strip, the Crew panel and the host's progress derivation are unchanged.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `work-progress`: "Progress in the sidebar tree" applies to the project row alone.

## Impact

- `crates/workspace-desktop/src/sidebar.rs`: the `row` layout, `session_row` and `ticket_row`.
- `docs/PRD.md`, `design/README.md`, `design/mockups/current` (the simulated tree).
- No host, core or schema change.
