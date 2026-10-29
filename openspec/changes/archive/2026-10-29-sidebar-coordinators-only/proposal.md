## Why

The Agents panel now lists every ticket and agent of a project, so the sidebar no longer has to. Listing them twice made the tree long and busy. The approved `agents-window` mockup keeps one row per project in the sidebar and sums up its agents on that row, raising only the extreme case of a stuck agent.

## What Changes

- **Projects only.** The sidebar tree lists groups and projects. Ticket rows and agent rows go, archived and legacy ones included, and so do the project row's chevron and per-project collapsing. Group collapse, renaming and drag reordering stay.
- **Agents line.** A project row's last line reads "N agents working" when any are, with a yellow "N agent(s) stuck" pill when the stuck rule flags any of its unarchived agents. The row's tooltip names the stuck count too.
- **Selection.** A project row stays selected while its coordinator or any of its agents is open.
- **Rollup.** A collapsed group counts stuck agents as urgent, after needs-you and before blocked.
- **What the ticket rows carried moves to the ticket cards.** The Agents panel's ticket card shows the PR line, the waived acceptance and a publish step waiting for approval, which only the sidebar's ticket rows showed. An agent's "1 question" figure leaves the tree with its row; its badge still names the questions.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `native-workspace`: the sidebar lists projects only, with the agents line, the stuck pill and the selection rule; the group rollup and drag reordering lose ticket, agent and project-collapse wording.
- `work-progress`: progress in the sidebar is the project row's alone, and the Agents panel's previous agents no longer refer to the tree.
- `ticket-workspaces`: ticket workspaces are listed in the Agents panel, not the sidebar.
- `ticket-publishing`: a waiting publish step shows on the ticket's Agents panel card.
- `pr-watch`: a ticket's PR line shows on its Agents panel card.

## Impact

- `crates/workspace-desktop/src/tree.rs`, `sidebar.rs`, `agents_panel.rs`, `assets.rs`, `wording/tickets.rs`.
- `docs/PRD.md`, `docs/DEVELOPMENT.md`, `design/README.md`, `design/brand.md`.
- No host, protocol or preference-file change: collapsed projects were never saved.
