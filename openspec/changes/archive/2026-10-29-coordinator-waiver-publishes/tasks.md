## 1. Host

- [x] 1.1 Share one check of a blocked waiver (every verifier checked, nothing untriaged) between `accept_ticket` and `publish_ticket`.
- [x] 1.2 Accept `publish_ticket`'s `waived` after a blocked cycle, from the coordinator only.
- [x] 1.3 Run a waived publish without asking in an allowed owner's repository, for open findings and for unreviewed commits at the cap; keep asking without a waiver, where the owner is not allowed, and before another pull request after an ended one.
- [x] 1.4 Record the waiver on the ticket for later publishes and the PR keeper, and clear it when a new cycle starts.
- [x] 1.5 Say in the tool's result that the publish runs under the coordinator's waiver.

## 2. Desktop

- [x] 2.1 Word an open ticket's waiver block without "at acceptance".

## 3. Text, tests and docs

- [x] 3.1 Reword the communication contract (v10), the coordinator core (v4), the `publish_ticket` description and the blocked-cycle message, and update the contract tests.
- [x] 3.2 Test a waived blocked publish, a waiver at the cap, the coordinator-only and checked-and-triaged refusals, a not-allowed owner, the cleared waiver and the keeper's push.
- [x] 3.3 Update `docs/PRD.md`, `docs/DEVELOPMENT.md` and the repository-owners mockup caption.
