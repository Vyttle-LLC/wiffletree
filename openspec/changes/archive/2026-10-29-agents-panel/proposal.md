## Why

Tickets and their agents are moving out of the sidebar into a docked Agents panel, as in the approved `agents-window` mockup. Before the sidebar can stop listing them, every agent it lists has to be reachable from the panel, and an agent that needs a look has to stand out there.

## What Changes

- **Crew becomes Agents.** The panel is renamed, and its toggle moves from the coordinator's strip to the header of every conversation of the project, reading "Agents · N working".
- **Previous agents.** Below the tickets, a folded "Previous agents (N)" section lists every other agent the sidebar lists under the coordinator: archived agents, agents of accepted or closed tickets, workers from before tickets and retired repository coordinators.
- **Stuck agents.** An agent is stuck when it is disconnected, or idle after a turn that ended in an error. Blocked is not stuck. A stuck agent's row and its ticket's card take the warning color, with the reason.
- **Agents card.** The coordinator's conversation pins one card above the composer, "N agents working on M tickets · K stuck", with "Open Agents".
- **Breadcrumb.** An agent's header links back to the coordinator, "← <project> / <ticket>", and its title names the agent.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `work-progress`: the Crew panel becomes the Agents panel, the strip loses its crew button, and stuck agents, the agents card and the breadcrumb are added.

## Impact

- `crates/workspace-desktop/src/agents_panel.rs` (was `crew_panel.rs`), `conversation_header.rs`, `conversation.rs`, `inspector.rs`, `shared.rs`, `progress.rs`, `main.rs`, `wording/tickets.rs`.
- `docs/PRD.md`, `docs/DEVELOPMENT.md`, `design/README.md`, `README.md`.
- No host or protocol change.
