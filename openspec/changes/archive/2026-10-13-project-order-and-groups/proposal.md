## Why

The sidebar lists every project newest first and nothing can be moved. Related projects end up scattered, and a shared first word such as "wiffletree:" pushes the part of each name that differs out of view. The human approved the study in `design/mockups/current/project-groups.html`: drag projects into any order, and show projects whose names share a first word together under that word.

## What Changes

- **A manual order, owned by the host.** The host stores one flat order of projects and returns them in it. A new project lands at the top. One command moves a project, or a block of projects, to just before another project or to the end. The desktop sends the command and redraws from the snapshot, so the order survives a relaunch and is the same for every client.
- **Store schema 7.** Projects gain a position. An existing store keeps the newest-first order it was drawn in until something is moved.
- **Groups by first word, drawn by the desktop only.** With grouping on, two or more shown projects whose names share a first word sit under one group row at the place of the topmost of them. Inside a group a row drops the repeated word. A collapsed group shows the most urgent state inside it. Groups are never created, renamed or deleted; renaming a project is how it changes group.
- **A Group button** in the PROJECTS header, left of Archived, on to begin with. Its state is saved with the desktop's other view choices in `desktop-preferences.json`, and toggling it never changes the stored order.
- **Drag and drop.** With grouping off, any project row moves anywhere in the flat list. With it on, a project moves within its own group, and groups and ungrouped projects move among each other; a group moves all its members as one block. An insertion line shows where the row lands and the dragged row dims.
- **Out of scope.** Manual or nested groups, dragging a project into another group, menu items or keyboard shortcuts for moving, a Settings entry, and any change to MCP tools or agent-facing context.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `workspace-host`: adds the stored project order and the command that changes it.
- `native-workspace`: adds first-word groups, the Group button and reordering by drag and drop.

## Impact

- `crates/workspace-core/src/lib.rs`: `Command::MoveProjects`.
- `crates/workspace-host/src/order.rs` (new) and `lib.rs`: schema 7, the order projects are read in, where a new project lands, and the move.
- `crates/workspace-desktop/src/tree.rs`, `sidebar.rs`, `preferences.rs`, `main.rs`: groups, the Group button, drag and drop.
- `docs/PRD.md`, `docs/DEVELOPMENT.md`, `design/README.md`, `design/mockups/current/project-groups.html`.
- `crates/workspace-desktop/src/automation.rs` and `scripts/drive_desktop.py`: development-only `press`, `drag` and `release` steps, so a drag can be driven and captured without a person's pointer.
- An older build refuses a schema 7 store, as it does any newer schema.
