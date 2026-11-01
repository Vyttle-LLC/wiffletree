## 1. Host

- [x] 1.1 `record_verdict` archives a verifier when its result is `passed`, and the failed round stops archiving passes.
- [x] 1.2 Tests: a verifier retires on its pass with the others and the cycle unaffected, through a failed round; a changed worktree leaves an archived pass standing; only a pass retires a verifier in its round.

## 2. Desktop

- [x] 2.1 `Roster`: `TicketCard`, `finished`, and finished tickets read from the index.
- [x] 2.2 The "Finished tickets (N)" fold, the heading's counts and the card of a finished ticket.
- [x] 2.3 The quiet line and `waiting_for`, `finished_outcome` and `ticket_counts` in `wording/tickets.rs`.
- [x] 2.4 Tests: the roster split, a project with every ticket finished, each PR outcome's line, the waiting wording, a stuck agent keeping its full row.

## 3. Docs

- [x] 3.1 `ui/mockups.html`, `design/README.md`, `docs/PRD.md`, `docs/DEVELOPMENT.md`.
