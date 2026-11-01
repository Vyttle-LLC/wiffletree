Four stacked tickets in the wiffletree repository, each with a full review. Pull requests may be opened; only Michael merges. Stage 4 archives this change.

## 1. Cascade core and Behaviors

- [x] 1.1 This change: proposal, design (from the grill's note), spec deltas, these tasks, and the study under `ui/mockups.html`, its CSP and no-script rule kept.
- [x] 1.2 `workspace-core::cascade`: `Layer`, `Resolved` with its provenance and notes, the `Setting` trait, `resolve` and `check`; the tightening rule for a Space and a repository. Unit tests: each layer overriding the one before, a disallowed layer, a looser safety value refused at save and ignored at read.
- [x] 1.3 Storage: `settings/behaviors.json` in the shared folder, checked by sync and named "Behaviors"; `behaviors:` and `repository_settings:` in `space.yaml`, the additions kept under the key `instructions:` that older hosts read. Round-trip tests for unknown keys and for additions stored as `instructions`.
- [x] 1.4 The catalog in `workspace-host::behaviors`, versioned with the stock skills; the Behavior passages leave Coordinating (v6), Implementing (v3), Reviewing (v3) and Assisting (v2). A test that no sentence is still in its stock text.
- [x] 1.5 Composition for every turn, compaction and context measurement through `session_instructions`: core, skill text, Behaviors, then Workspace, Space and Repository additions; Space additions reach every role. Composition tests for an implementer and a coordinator in Sage, and a repository in two Spaces.
- [x] 1.6 Enforcement: Publishing "always ask first" (`PublishRequest.always_ask`, `PublishAsk::AlwaysAsk`) and Review depth "full review for every ticket" in `verify_ticket`, with tests.
- [x] 1.7 Host commands `behavior_catalog`, `layer_values`, `set_layer_values` (human only) and `effective_settings`; the coordinator's `workspace_context` `behaviors`; `docs/host-protocol.md`. A sync test for a Behavior saved on one Mac.

## 2. Models cascade

- [x] 2.1 `space.yaml` `models.rows`: a Space overrides any row of the Who runs what table, in the table's row shape; `models.threads` and `models.jobs` read as the Assistant and Job rows. Resolve recipe model → Space row → this Mac's row with `cascade::resolve`, falling through an unavailable provider with a note.
- [x] 2.2 A Space overrides the round cap, the full-review cap and the levels its coordinator may use; it may not drop a verifier with an open finding.
- [x] 2.3 Retire `SpaceLocal.models`, with a one-time note for the Spaces page.
- [x] 2.4 `workspace_context`'s `model_selection` resolved for the project's Space, with provenance on overridden rows.

## 3. Behaviors page

- [x] 3.1 The Skills page becomes Behaviors: the scope bar `Workspace · Space ▾ · Repository ▾`; per role, each Behavior with its value, a provenance chip and Reset; the scope's additions. `Intent::OpenBehaviors` opens it at a scope for stage 4's Space page link.
- [x] 3.2 Advanced (Workspace only): today's rewrite, history and merges.
- [ ] 3.3 Offer once to delete additions lines a Behavior now says; never remove them unasked.

## 4. Models, Spaces and Settings screens

- [x] 4.1 Models: the scope bar `This Mac · Space ▾`, overridden rows marked, Reset per row; the Space-resolved Right now and fallback banner; Reviews in the Space.
- [x] 4.2 The Space page: models and instructions leave it; links to "Behaviors for ‹Space›" and "Models for ‹Space›"; the brain's compile time and gate shown read-only; the composer's model label and the page's fallback note resolve the Space's Assistant row.
- [x] 4.3 The Settings window's groups, "Shared with your Macs", "Spaces" and "This Mac".
- [x] 4.4 Effective settings from a project's header, from `effective_settings`.
- [ ] 4.5 Run `openspec validate --strict`, sync the specs and archive this change: a follow-up once stages 3 and 4 have both merged, so neither pull request edits an archived change.
