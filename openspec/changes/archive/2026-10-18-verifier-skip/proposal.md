## Why

When a reviewer's subscription is spent, its "then" choice today is a model on the other subscription or wait. A wait holds the whole review, and its verification slot, until the reset. The human wants a third choice for reviews they can do without for a while: skip, so the others run and the review goes ahead without it. The human approved the study `mockup-skip-review.html` and chose that a review that passed with a skip publishes the same way as a full pass.

## What Changes

- **A third then choice.** A verifier row's then, on its main or small-work line, can be skip. Coordinator, Implementer, Tester and PR keeper rows keep model or wait, and a save with skip on them is refused. Tables saved before skip load unchanged.
- **Decided once, when a cycle is created.** A verifier whose row says skip, and whose first choice is spent or not signed in, starts no session and is recorded on the cycle as skipped with a one-line reason. It takes no part in later rounds, never blocks and holds no slot. A verifier with an open finding waits instead. A queued cycle decides its skips when it is created.
- **Passing.** A cycle passes when every verifier that ran passed. Accepting and publishing treat it as any passed cycle.
- **Something must run.** If every verifier of the level would skip, `verify_ticket` refuses with the reasons and a PR keeper review does not start, so its commit is not pushed.
- **Telling people.** The cycle's outcome message names each skipped verifier and why, a passed PR keeper review with a skip sends it too, the Tickets inspector shows a Skipped pill, and `workspace_context` shows `"then": "skip"` and that a row would skip right now.
- **Models page.** The then menu of a verifier row offers "Skip this review"; the cell reads "skip"; Right now shows a grey "Skips · <reset>" pill.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `model-routing`: the table's then choice can be skip on verifier rows, and resolution may skip a review's verifier.
- `ticket-verification`: cycles record skipped verifiers, pass without them, refuse to start when all would skip, and name them in the outcome.
- `native-workspace`: the Models page's skip menu item and Skips pill.

## Impact

- `crates/workspace-core/src/selection.rs`: `Then` (wait, model, skip) on `Line`; `resolve`'s `may_skip`; `Pick.skip`; `validate`.
- `crates/workspace-core/src/lib.rs`: `Verification.skipped` and `SkippedVerifier`.
- `crates/workspace-host/src/verification.rs`: `verifier_picks`, `start_cycle`, the outcome message; `live.rs`, `policies.rs`, `mcp.rs`, `skills/core/coordinator.md`.
- `crates/workspace-desktop/src/models.rs`, `models_view.rs`, `team_view.rs`.
- Docs: `docs/PRD.md`, `docs/DEVELOPMENT.md`, `README.md`.
