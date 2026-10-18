## Why

Choosing models takes five settings today: a Providers allowlist of exact model and effort pairs, a Roles card of providers per role, a prose guide the coordinator reads to pick a model for every assignment and every verifier, an optional provider per verifier, and a separate PR keeper model. The human thinks about it more simply: each job has a model it should run on, and a second one to use when the first subscription is nearly used up. Today nothing moves when a subscription runs low; agents start on a provider whose weekly limit is at 99% and fail. The human approved a mockup that replaces all five with one "Who runs what" table and a single switch threshold (`ui/models.html`, upper half).

## What Changes

- **BREAKING:** remove the Providers allowlist (`ProviderAccess`, `AllowedModel`, `enabled`), providers per role (`role_providers`, `PROVIDER_ROLES`), `ModelSelection::{permits, permits_verifier, providers_for, prefill, allowed}`, `Rejection`, the verifier `provider` field and the separate `keeper` model.
- Add the **Who runs what** table, saved per machine: one row per job (Coordinator, Implementer, Tester, one per configured verifier keyed by its focus, and PR keeper). Each row has a first choice (provider, model, effort), an optional "then" choice (none means wait) and an optional small-work line with its own first and then.
- Add one **switch threshold**, default 95%, shared between machines in `settings/models.json`.
- **When a provider counts as spent:** its latest quota reading has a window at or over the threshold whose reset has not passed, or one at or over it with an unknown reset, or a window that reported the limit reached. No reading means not spent. A provider whose CLI is missing or signed out cannot be chosen. Readings come only from the providers' own telemetry.
- **Resolution when an agent starts:** `assign_ticket`, each verifier a cycle starts (the PR keeper's reviews included), a new project coordinator and each PR keeper model run. First unless spent, then then unless spent; when every choice is spent, the one whose reset comes earliest is pinned and its turns wait for that reset. A running agent never switches.
- Provider-lens reviewers default to "then: wait", so a Claude review waits for Claude rather than running on Codex.
- Comment sorting and skill merges use only the PR keeper row's Claude choice; sorting waits while Claude is spent.
- **BREAKING (MCP):** `assign_ticket` and `verify_ticket` drop `profile`, `reason` and `verifiers`, and take an optional `small`. `assign_ticket` takes `like: <verifier focus>` for an ad-hoc reviewer. `workspace_context.model_selection` becomes the table, what each row would use right now and why, the threshold, the review levels with caps, and the guide as optional notes.
- Each agent records its profile, the chooser Wiffletree, the revision and a plain reason, shown where "Chosen by …" shows today. Human overrides still win; the new-agent dialog is pre-filled from the row.
- The Models page becomes Subscriptions, When a subscription runs low, Who runs what, Reviews and Notes, per the mockup.
- An idempotent `who-runs-what` migration seeds the table from the mockup's values on the enabled providers, keeps a saved PR keeper model, removes the migrated-tiers section from the guide and leaves a one-time note on the Models page.
- The PRD's model-routing, limits, reliability, shared-settings and PR keeper text is rewritten to match. Paid fallback and reset credits stay forbidden.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `model-routing`: the table, the threshold, the spent rules, resolution at agent start, waiting for the earliest reset, the PR keeper row, the suggested setup and the migration replace providers, roles, coordinator selection and held turns for disabled providers.
- `workspace-host`: the assignment tools ask for a job and optional small work, not a model; `workspace_context` carries the table.
- `native-workspace`: the Models page, the new-agent picker and the visible reason.
- `ticket-verification`: verifiers lose their provider; `verify_ticket` and the PR keeper's reviews take each verifier's model from its row.
- `pr-keeper`: the keeper's model is the table's PR keeper row; sorting needs its Claude choice.
- `settings-repository`: the threshold is shared; the table stays on each machine.

## Impact

- `workspace-core`: `selection.rs` (table, threshold, spent rules, resolution, reasons), `suggestion.rs` (seeded rows), `models.rs` (short model names), `lib.rs` (`VerifierConfig`, `Command`, `check_sorting_model`, `check_tries`), tests.
- `workspace-host`: `policies.rs` (migration, holds, resolution), `live.rs` (tools, context, keeper), `verification.rs`, `keeper.rs`, `skills.rs`, `settings.rs`, `shared.rs`, `sync.rs`, `mcp.rs`, `service.rs` and `service/scheduling.rs` (waiting turns, detection), `skills/core/coordinator.md`.
- `workspace-desktop`: `models.rs`, `models_view.rs`, `verifiers.rs`, `creation.rs`, `conversation.rs`, `shared.rs`, `welcome.rs`, `team_view.rs`. `repositories_view.rs` is untouched.
- `docs/PRD.md`, `README.md`, `docs/DEVELOPMENT.md`.
- No new dependencies. No provider is called for discovery; quota readings are the existing telemetry.
