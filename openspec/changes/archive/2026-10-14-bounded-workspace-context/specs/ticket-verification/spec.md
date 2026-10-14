## MODIFIED Requirements

### Requirement: The ledger is visible to the coordinator and the human
`workspace_context` SHALL include each open ticket's ledger, earlier cycles and waiver reason. It SHALL show an accepted or closed ticket as one short entry holding only its id, title, state, repository, branch and, if it has one, its pull request's number, state and URL; the full record SHALL stay available through the optional `ticket_id` argument, which SHALL return that one ticket of the caller's project in full, whatever its state, and SHALL refuse a ticket of another project. The Tickets panel SHALL show, on each ticket's card, every cycle's verification lines, any waiver reason, and a "Decision ledger" section listing each entry's id, summary, location, source, status and reason.

#### Scenario: Coordinator reads the ledger
- **WHEN** the coordinator calls `workspace_context` after a cycle with three findings
- **THEN** that ticket's entry lists the three ledger entries with their ids, statuses and sources

#### Scenario: A finished ticket is one line
- **WHEN** the coordinator calls `workspace_context` and a ticket is closed with a merged pull request
- **THEN** that ticket's entry holds only its id, title, state, repository, branch and the pull request's number, state and URL

#### Scenario: A finished ticket by id
- **WHEN** the coordinator calls `workspace_context` with the `ticket_id` of a closed ticket
- **THEN** the result is that ticket's full record, with its brief, cycles, ledger and pull request

#### Scenario: A ticket of another project
- **WHEN** the coordinator calls `workspace_context` with the `ticket_id` of another project's ticket
- **THEN** the call fails and returns nothing of that ticket

#### Scenario: Human reads the ledger
- **WHEN** the human opens the Tickets panel for a ticket with two cycles and five ledger entries
- **THEN** the ticket's card shows both cycles' lines and all five entries with their status and reason

## ADDED Requirements

### Requirement: The coordinator's context stays bounded as a project ages
`workspace_context` SHALL show an archived agent in `team` as a short entry holding only its id, parent, name, role, status and `archived`, and SHALL leave an archived child's reports out of `child_reports`. Open tickets and live agents SHALL keep their full entries.

#### Scenario: An aged project stays small
- **WHEN** a project has twenty closed tickets with long briefs and ledgers, 120 archived agents and one open ticket, and the coordinator calls `workspace_context`
- **THEN** the reply is far under 256 KB and the open ticket is still in full

#### Scenario: Archived agents are short
- **WHEN** an archived agent has reported to the coordinator and the coordinator calls `workspace_context`
- **THEN** `team` lists it by id, parent, name, role, status and `archived` only, and `child_reports` has nothing of it
