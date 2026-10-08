## 1. Core types (workspace-core)

- [x] 1.1 Add `ModelSelection` (providers, role providers, reviews, guide, revision), `ProviderAccess`, `AllowedModel`, `Reviews` and `Selection`/`Chooser`. Add `ModelSelection::{validate, permits, prefill}`, where `permits` returns distinct wrong-provider and not-allowed errors. Add `selection` to `SessionRuntime` and `model_selection` to `Snapshot`.
- [x] 1.2 Slim `RolePolicy` to `role` and `turn_budget_minutes`. Delete `Complexity`, `RoutingMode`, `ProviderProfiles`, `RoleDefault`, `Route`, `select`, `Command::Route` and `Command::SetRoleDefaults`. Add `Command::SetModelSelection`. Change `Command::AssignTicket`/`CreateSession` from `provider` to `profile`.
- [x] 1.3 Replace `tests/routing.rs` with tests for:
  - `permits`: wrong provider; disabled provider; unticked model; allowed; and a reviewer on any enabled provider, whatever the review providers;
  - `prefill`: Claude-then-Codex order, the reviewer's first review provider and the first allowed model;
  - every `validate` rejection.

## 2. Migration (acceptance criterion 3)

- [x] 2.1 Move the legacy policy types and built-ins into a private `legacy` module in `policies.rs`. Make `migrate_opus_defaults` use them and skip slim rows; its behavior is otherwise unchanged.
- [x] 2.2 Add the `model_selection` table and `migrate_model_selection`: fresh-store record, pinning of unpinned sessions, the per-field conversion, starter-guide generation, slim policy rows and the `policy_migrations` id. Run it after the Opus migration in `Host::open`.
- [x] 2.3 Add a host test for each shape, seeded from raw policy JSON: legacy fixed Codex (including maintenance), built-in fixed Opus, custom fixed, automatic without Big/Small, automatic with Big/Small (today's real data), and a row without `turn_budget_minutes`. Each test asserts:
  - provider models are the per-provider union, and providers are enabled exactly when they have models;
  - role providers come from the default provider for the coordinators, implementer and tester;
  - `reviews.providers` comes from the reviewer's default provider, or both providers when both have Big/Small profiles, and `reviews.instructions` is empty;
  - turn budgets are kept;
  - the guide states every role's Big/Small sizing per provider, its tiers and its fixed mode.
- [x] 2.4 Host tests for pinning:
  - unpinned sessions are pinned to their previous profile;
  - already-pinned sessions are untouched;
  - a provider-mismatched main coordinator stays unpinned and its next turn takes its own provider's first allowed model; any other unpinned agent is held.
- [x] 2.5 Host test: a fresh store equals the migration of the built-in policies.
- [x] 2.6 Host test: re-opening a migrated store after the human edited providers, roles and guide leaves the selection row, runtimes and policy rows byte-identical.

## 3. Assignment and enforcement (acceptance criteria 1 and 2)

- [x] 3.1 Replace `assignment_route` with `permits`. Update `assign_ticket` and `create_repo_coordinator` per design "MCP arguments":
  - reject the removed keys;
  - validate the reason;
  - record the selection;
  - log `model_selected`/`model_rejected` activity;
  - return the profile and selection.
- [x] 3.2 Host tests for assignment:
  - an allowed assignment records profile, reason, coordinator and revision for both tools;
  - a wrong provider for the role and an unticked model are each rejected with their own message, no session created, no substitution and a `model_rejected` activity;
  - a missing or multi-line reason, and `size`/`complexity`/`provider`, are rejected;
  - a repeated assignment returns the existing agent unchanged.
- [x] 3.3 Human paths:
  - `CreateSession`/`AssignTicket` take a profile;
  - `CreateProject` pins the main coordinator from `prefill`;
  - `ConfigureSession` records `Human`.

  Test that a human choice outside the role's providers succeeds and records the human.
- [x] 3.4 Replace the remaining `select` calls in `service.rs::start_turn` and the simulator with the pinned profile, or for an unpinned session its provider's first allowed model ("Choose a model for this agent" when that provider is disabled). Record `selection` and `model_selection_revision` in the turn detail.
- [x] 3.5 Held turns: at turn start for coordinator-chosen sessions, check that the provider is still enabled. A failed check leaves the session Blocked with input queued and logs a `turn_held` activity. Tests cover:
  - provider disabled: held, input kept;
  - provider enabled: runs;
  - human-chosen on a disabled provider: never held.
- [x] 3.6 Parent wake: when a turn is held, deliver a `blocked` report to the parent on the agent's behalf, with `last_error` as its text and the id `report:‹agent›:turn-held:‹first held message id›`. Test that the parent receives exactly that report and is scheduled to wake, and that a retry without a fix sends no duplicate.
- [x] 3.7 `workspace_context` adds `model_selection` for both coordinator roles: enabled providers with allowed models, role providers, review settings, guide and revision. Test that workers get none of it and that an edit appears on the next call.
- [x] 3.8 Rewrite the affected tests in `tests/policies.rs`, `tests/communications.rs` and `tests/recovery.rs` for the new arguments.

## 4. MCP, skills and docs (acceptance criterion 5)

- [x] 4.1 Update the `assign_ticket` and `create_repo_coordinator` schemas and descriptions in `mcp.rs`. Leave non-model arguments untouched.
- [x] 4.2 Rewrite the model paragraphs of `repository-coordinator.md` and `main-coordinator.md` (v4). They will say:
  - pick a provider configured for the role (any enabled provider for a reviewer), then the model and effort by the guide;
  - give a reason;
  - when the tester reports passed, start the verification set in parallel on the same ticket: one reviewer per standard-review provider (focus "Claude", "Codex"), plus one reviewer per custom review in the review instructions (focus such as "Codex · reviso:style"), passing that instruction text to it.

  Check the other skills for size/Big/Small wording.
- [x] 4.3 Rewrite the PRD's "Role configuration and bounded model routing" section and acceptance list, and update the README if it mentions tiers.

## 5. Desktop (acceptance criteria 1 and 4)

- [x] 5.1 Rewrite the `models.rs` helpers and their tests for provider model grids (catalog plus saved values), role providers, review settings and pre-fill. Delete the Big/Small helpers.
- [x] 5.2 Rewrite `models_view.rs` as four cards with one Save/Discard and inline validation errors, per design "UI (a)" and `ui/mockups.html`:
  - Providers: enable toggle, CLI status from `ProviderCheck`, model × effort checkboxes;
  - Roles: provider checkboxes for coordinators, implementer and tester;
  - Review: provider checkboxes with "One review per ticked provider, run in parallel after the tester." and an instructions box with the reviso:style example as placeholder;
  - Guide: Markdown editor.
- [x] 5.3 Add the model picker to the agent and repository-team creation dialogs: pre-filled, with the role's providers grouped first and other models after ("UI (c)").
- [x] 5.4 Show the selection line above the composer's model pills, and each agent's model and reason in the Tickets panel, including reviewers by focus ("UI (b)"). Replace the `policy.default` fallbacks in `main.rs` and `conversation.rs`.
- [x] 5.5 Check that both rejection kinds render as a failed `assign_ticket` step and a `model_rejected` event ("UI (d)"). Verified for Events (`ui/rejection-events.png`); the failed work-stream step is the existing failed tool-call rendering and was not captured, because that needs a real provider turn.
- [x] 5.6 Build and launch the app against a copy of a real store and screenshot each surface from 5.2–5.5 into `openspec/changes/model-selection/ui/`:
  - `models-page.png` and `models-page-review-guide.png` (a migrated copy of the real store)
  - `creation-picker.png` and `creation-picker-reviewer.png`
  - `agent-reason.png`
  - `tickets-reasons.png`
  - `rejection-events.png` (both rejection kinds)

  Report the paths to the coordinator as `progress` before testing finishes. Check that edits survive a restart.

## 6. Validation

- [x] 6.1 Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
- [x] 6.2 Run `openspec validate model-selection --strict`, and record any acceptance item not verified interactively.

## 7. Rebase onto the flattened hierarchy

- [x] 7.1 Drop review settings, `RolePolicy`, `SetPolicy`, `create_repo_coordinator` model arguments and the repository-coordinator role provider.
- [x] 7.2 `VerifierConfig` drops `size`; default verifiers are a tester, Reviewer · Claude and Reviewer · Codex.
- [x] 7.3 `verify_ticket` takes a profile and reason per verifier, checked by `permits_verifier`, recorded as a `Selection`; tests cover a pass and each rejection.
- [x] 7.4 The Review card on the Models page edits the verifier list, with the turn-limit note; Settings loses its verifier editor.
- [x] 7.5 The migration runs after schema 6; tests cover a pre-flatten store with a repository-coordinator row and saved verifier sizes.
- [x] 7.6 Update `main-coordinator.md`, the MCP schemas, the PRD and the development guide.
