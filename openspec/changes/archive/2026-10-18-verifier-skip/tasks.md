## 1. Core

- [x] 1.1 Replace `Line.then: Option<ModelProfile>` with `Then` (wait, model, skip), saved as before or as `"skip"`, so old tables load unchanged.
- [x] 1.2 Refuse skip on Coordinator, Implementer, Tester and PR keeper rows in `validate`.
- [x] 1.3 Give `resolve` a `may_skip` flag and a skip pick with a present-tense reason and the past-tense reason the cycle records.
- [x] 1.4 Add `Verification.skipped` and mention skipped verifiers in the passed and review lines.

## 2. Host

- [x] 2.1 Let only a cycle's verifiers without an open finding skip, and refuse a level whose every verifier would skip, for `verify_ticket` and the PR keeper.
- [x] 2.2 Record skipped verifiers in `start_cycle` without starting a session; a queued cycle keeps the skips it was created with.
- [x] 2.3 Name skipped verifiers in the outcome message, and send it for a passed PR keeper review that skipped one.
- [x] 2.4 Show skip in `workspace_context` and describe it in the tool descriptions and the coordinator skill.

## 3. Desktop

- [x] 3.1 Offer "Skip this review" on verifier rows' then menus, read the cell as "skip", and show the grey Skips pill with its tooltip.
- [x] 3.2 Update the subscription card's sentence and show a Skipped pill in the Tickets inspector.

## 4. Tests and docs

- [x] 4.1 Test resolution, validation and saved JSON in core; a skipped cycle passing, every verifier skipping and an open finding waiting in the host; the keeper's refusal and message; the Skips state on the Models page.
- [x] 4.2 Update `docs/PRD.md`, `docs/DEVELOPMENT.md` and `README.md`.
