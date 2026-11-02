## Why

The sidebar lists every project in one list, whatever its Space, so Vyttle, Sage and Personal projects are mixed together and only names sharing a first word sit together. Michael approved the static study `design/mockups/current/projects-by-space.html`: a section for each Space, with the existing first-word groups inside it.

## What Changes

- Each Space with a shown project gets a section row with a chevron, its name in uppercase and its project count, in the order of Settings › Spaces, followed by No Space for projects in none. Sections show whether grouping is on or off.
- A section collapses from its chevron, in memory only, and collapsed shows the same rollup as a collapsed group. Selecting a project inside a collapsed Space or group opens it.
- First-word groups form only within one Space.
- A drag stays within the dragged row's Space. Over another Space no insertion line shows, a drop does nothing, and over that Space's row a hint says "Projects move between Spaces from their Overview."
- The host stores nothing new: the one stored order is filtered by Space.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `native-workspace`: Space sections in the project list, groups scoped to a Space, and drags kept within a Space.

## Impact

- `crates/workspace-desktop/src/tree.rs`, `crates/workspace-desktop/src/sidebar.rs`.
- `docs/PRD.md`, `docs/DEVELOPMENT.md`, `design/README.md`, `design/mockups/current/projects-by-space.html`.
