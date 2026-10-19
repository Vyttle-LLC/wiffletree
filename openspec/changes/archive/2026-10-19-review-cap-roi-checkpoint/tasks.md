## 1. Host

- [x] 1.1 Replace the cap refusal's "Only the human can raise the limit" with the ROI guidance.
- [x] 1.2 At the cap, offer asking the human only for a product, security or data-loss decision in the blocked cycle's actions.
- [x] 1.3 Reword the coordinator core (v3) and narrow the Coordinating stock skill's security decision (v2).

- [x] 1.4 Let a waiver at the cap cover named unreviewed commits in `accept_ticket` and `publish_ticket`, and record them.
- [x] 1.5 Make such a publish always wait for the human, with the commits and reason on its item.

## 2. Desktop

- [x] 2.1 Reword the "Full reviews per ticket" caption.

## 3. Tests and docs

- [x] 3.1 Assert the refusal does not mention raising the limit, and the blocked message's qualified "ask the human".
- [x] 3.2 Update the contract tests and the skills and sync fixtures for the new versions.
- [x] 3.3 Update `docs/PRD.md`.
- [x] 3.4 Test accepting and publishing with a waiver of unreviewed commits, at and below the cap.
