## 1. Documents

- [x] 1.1 Copy the approved mockup to `ui/models.html`.
- [x] 1.2 Rewrite the PRD's "Role configuration and bounded model routing", "Limits and scheduling", "Reliability and control", "Settings shared between machines" and PR keeper text to match; keep paid fallback and reset credits forbidden. Update README.md and docs/DEVELOPMENT.md where they describe the allowlist.

## 2. Core (workspace-core)

- [x] 2.1 Replace `ModelSelection`'s providers, roles and keeper with `jobs: Vec<JobRow>`, `switch_at`, `notes` and `copied`; add `Job` (with `Job::for_role`), `Line`, `JobRow`, `validate`, `fit`, `row`, `set_keeper`, `keeper`, `sorting_model`. Remove `ProviderAccess`, `AllowedModel`, `Rejection`, `PROVIDER_ROLES` and `VerifierChoice`; drop `VerifierConfig.provider`.
- [x] 2.2 Add `Limits::spent` and `resolve` with their reasons, and `Chooser::Wiffletree`; add `SessionRuntime.waiting`.
- [x] 2.3 `suggest` builds the seeded table from the usable providers and the catalog; add the seed rows and closest-model mapping used by the migration.
- [x] 2.4 `check_sorting_model` and `check_tries` take the PR keeper row.
- [x] 2.5 `Command`: `SetModelSelection` carries the table; add `DismissModelNotes`.
- [x] 2.6 Tests: spent rules (threshold, passed reset, unknown reset, no reading, limit reached), resolution order (small work, wait, signed out, earliest reset), validation, `fit`, the suggestion.

## 3. Host (workspace-host)

- [x] 3.1 `who-runs-what` migration in `policies.rs`: seed, keeper, guide section, note; idempotent. Tests: Michael's machine, one provider, keeper kept, re-run, restart.
- [x] 3.2 The threshold in `settings/models.json`: shared store, `check`, sync label; merged into `model_selection()` with the guide; saved with the table.
- [x] 3.3 Record detection results; `Host::limits()`; `resolve_job`; pin with `Chooser::Wiffletree`.
- [x] 3.4 `assign_ticket` and `verify_ticket` take `small` and `like`, refuse the removed arguments; `create_project`, keeper reviews and keeper runs resolve their rows; `workspace_context.model_selection` is the table.
- [x] 3.5 Waiting: `hold_reason` for waiting sessions, the Wiffletree message to the parent, the wake at the reset and at a quota reading while a session waits, `launch` clears the flag.
- [x] 3.6 Sorting and skill merges use the PR keeper row's Claude choice; sorting waits while Claude is spent.
- [x] 3.7 MCP schemas and descriptions; the coordinator's fixed core.
- [x] 3.8 Tests: tool schemas and refusals, a switched implementer and its reason, a running agent not switched, the Claude reviewer waiting, every choice spent, sorting never on Codex.

## 4. Desktop (workspace-desktop)

- [x] 4.1 Models page per the mockup: Subscriptions, When a subscription runs low, Who runs what with Right now, Reviews, Notes; one-time and copied-row notes.
- [x] 4.2 Verifier editor without the provider field.
- [x] 4.3 New-agent picker pre-filled from the row; "Chosen by Wiffletree" reasons; detection at launch on every store.

## 5. Finish

- [x] 5.1 `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- [x] 5.2 Archive the change and commit it with the code.
