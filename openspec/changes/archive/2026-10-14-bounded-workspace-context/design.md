## Context

The reply is built by `live.rs::agent_context`. `ticket_overview` serializes every ticket of the project in full (brief, every cycle, the whole ledger), `team` lists every session of the project, and `child_reports` lists the recent reports of every direct child.

## Decisions

- **A finished ticket is a short entry, not removed.** Its id, title, state, repository, branch and pull request keep the coordinator able to name it and find its PR. Its follow-ups and findings stay readable through `ticket_id`.
- **Archived agents stay in `team` as short entries; their reports leave `child_reports`.** A coordinator has no tool to restore an agent: the human restores it, and `resume_agent` refuses an archived one. So all the coordinator needs of an archived agent is its id and name to point the human at it, which the short entry keeps, with its parent_id because the coordinator double reads that field from every entry. A report is something to act on; an archived agent's reports were delivered before it was archived, and their findings live in the ticket's ledger, which `ticket_id` returns. The short entry is about a quarter of the size of a full one, so `team` still grows with the agents, slowly.
- **`ticket_id` returns the same record an open ticket has in the overview** (including its agents and branch warnings), so there is one shape for a full ticket. It refuses a ticket whose coordinator is in another project.
- **Open tickets and live agents are untouched**, so existing coordinators keep working.
