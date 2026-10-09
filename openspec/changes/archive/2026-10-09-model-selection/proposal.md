## Why

Model routing today is a grid of per-role Small/Standard/Complex tiers plus Big/Small profiles per provider. Coordinators pick a size, the host maps it to a model, and the recorded reason is only the tier's name. The human thinks about it differently. On one machine they have certain subscriptions. Each role should use certain providers ("Claude for implementation, Claude and Codex for review"). Within a provider, the agent should pick the model by the size of the work: Sonnet for a small change, Opus for a large one. On a machine with only one subscription, the configuration differs.

This change lands on top of `flatten-coordination-hierarchy`: one project coordinator, no repository coordinators, and `verify_ticket` starting every configured verifier. That verifier list replaces the review settings described in earlier revisions; see design "Revision after the flattened hierarchy".

## What Changes

- **BREAKING:** remove the per-role tiers (`Complexity`, `RolePolicy.small/standard/complex`, `RoutingMode`, `RolePolicy::select`), Big/Small `provider_profiles`, `RoleDefault` and the `Route`/`SetRoleDefaults` commands. `RolePolicy` and the `SetPolicy` command are removed; the legacy `policies` rows are read only by the migrations.
- Add per-machine **providers**. Claude and Codex can each be enabled or disabled, with allowed model and effort pairs. This is the hard limit on what coordinators may run.
- Add **providers per role** for the project coordinator, implementer and tester. Each role picks providers, not models, and the host enforces them.
- **Verifiers:** the `flatten-coordination-hierarchy` verifier list drops `size`, defaults to a tester, Reviewer · Claude and Reviewer · Codex, and is edited on the Models page's Review card instead of Settings.
- Add a prose **guide**, never parsed, on how to pick the model and effort within a provider by size and risk.
- **BREAKING (MCP):** `assign_ticket` replaces `provider`, `size`, `complexity` and `profile` with a required exact `profile` and a required one-line `reason`. `verify_ticket` gains a required `verifiers` list of `{focus, profile, reason}`, one per configured verifier, checked against the allowlist and the verifier's provider. The host rejects a provider that is not configured for the role, or a model that is not allowed, visibly and with no fallback. Other arguments are unchanged.
- Every agent records how its model was chosen. Its chat and the Tickets panel show the model and reason.
- Human choices win. The new-agent dialogs gain a model picker, pre-filled from the role's first provider and that provider's first allowed model. Neither the picker nor the chat override is limited.
- Turns of coordinator-chosen agents are held at turn start when their provider has been disabled on this machine. The host reports `blocked` to the parent on the agent's behalf.
- The Models page becomes four cards: Providers, Roles, Review and Guide.
- An idempotent `model-selection` migration, run after store schema 6, handles every saved shape, including the legacy `mode: fixed` and maintenance shapes:
  - Today's allowed lists are unioned per provider, which **loosens enforcement**.
  - Default providers become role providers.
  - Big/Small profiles, tiers, fixed mode and saved verifier sizes become guide prose.
  - Unpinned sessions are pinned first.
- The coordinator skills, MCP tool descriptions and the PRD's model-routing section are rewritten to match.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `model-routing`: tiered role policies and bounded automatic routing are replaced by per-machine providers, providers per role, the guide, exact coordinator selection with a reason, human-wins pickers, review settings, held turns for disabled providers with a parent wake, and the migration.
- `workspace-host`: the assignment MCP tools take an exact model and reason, and `workspace_context` carries the model and review configuration to coordinators.
- `native-workspace`: the Models page edits providers, roles, review settings and the guide; creation dialogs gain a model picker; agents show how their model was chosen; rejections are visible.

`ticket-verification` is also modified: verifiers take an exact model and reason. These capabilities are defined only in the unarchived `native-workspace-foundation` and `flatten-coordination-hierarchy` changes. Archive or sync that change before archiving this one, so the MODIFIED and REMOVED deltas have a base.

## Impact

- `workspace-core`:
  - new `ModelSelection`, `ProviderAccess`, `AllowedModel`, `Reviews` and `Selection` types;
  - a slimmed `RolePolicy`;
  - `Command` and `Snapshot` changes;
  - tier code and its tests removed.
- `workspace-host`:
  - `policies.rs`: the new migration, legacy decode types and `set_model_selection`;
  - `live.rs`: assignment tools, context and human creation;
  - `service.rs`: turn-start holds, the parent report and turn detail;
  - `lib.rs`: open order, commands and the simulation;
  - `mcp.rs`: schemas;
  - a new `model_selection` table;
  - skills `main-coordinator.md` and `repository-coordinator.md`.
- `workspace-desktop`: `models.rs` and `models_view.rs`, `creation.rs`, `conversation.rs`, `team_view.rs` and `main.rs`.
- `docs/PRD.md` model-routing section.
- `openspec/changes/model-selection/ui/mockups.html` is a static design study.
- No new dependencies and no provider calls. Existing agents keep their provider and model.
