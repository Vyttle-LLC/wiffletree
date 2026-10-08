## Revision after the flattened hierarchy

This change was rebased onto `flatten-coordination-hierarchy`. Where the rest of this document disagrees, this section wins:

- **No repository coordinators.** `create_repo_coordinator` and the repository-coordinator role are gone. The Roles card covers the project coordinator, implementer and tester (`PROVIDER_ROLES`). The coordinator skill is `main-coordinator.md` (v5).
- **No review settings.** `reviews {providers, instructions}` is dropped. The verifier list (`VerificationSettings`, in host `settings.json`) is the only definition of who verifies. `VerifierConfig` keeps `role`, `focus`, `instruction` and `provider`; `size` and `ProfileSize` are removed. The default list is Tester · Tests, Reviewer · Claude and Reviewer · Codex, which fits the default turn limit of 4. A style review is opt-in.
- **`verify_ticket { ticket_id, verifiers: [{focus, profile, reason}] }`.** One entry per configured verifier, matched by focus. Each is checked by `ModelSelection::permits_verifier`: the verifier's configured provider, then `permits` for its role. Every check runs before anything starts; a refusal logs `model_rejected` and starts nothing. A new verifier is pinned with a `Coordinator` selection, like an assignment; a verifier session that already exists under that focus is reused only when its pinned profile equals the submitted one; otherwise it is archived (`verifier_replaced`) and a fresh session starts on the submitted profile and reason, as in a first cycle. A pinned model never changes, and the call refuses if a reused verifier would be held. All of it happens in one transaction after every choice is validated. `max_rounds`, the pinned commit and the HEAD check on accept are unchanged. A verifier held mid-cycle reports `blocked` and the rest of its round runs, so the round ends `blocked` and wakes the coordinator.
- **Review card.** The Models page's Review card edits the verifier list and round cap and saves each change at once (`SetVerification`); Providers, Roles and Guide keep the page's Save. Settings no longer edits verifiers. The card notes projects whose turn limit is below the verifier count plus one; it never changes a limit.
- **Providers card.** Each enabled provider shows an Add row (a model dropdown that omits listed models, and Add, which allows the model at its first effort; the catalog reports no default effort) above one line per allowed model, with a checkbox per supported effort and Remove. Unticking the last effort removes the model. It replaces the model × effort grid in "UI (a)"; the data and enforcement are unchanged. Screenshot: `ui/models-providers.png`.
- **`RolePolicy` removed.** Turn budgets were removed on main, leaving nothing on the slim row, so `RolePolicy`, `Snapshot.policies` and `SetPolicy` are deleted. The legacy `policies` rows stay as they were; only the migrations read them, and a store with no rows is a fresh store.
- **Migration order.** `model-selection` runs in `Host::open` after `flatten::migrate` (schema 6). A `task_orchestrator` row's models join the union and its settings become guide prose; it gets no role providers. Saved verifier sizes from `settings.json` become a "Verifiers" guide section, stopping before the 32 KiB limit.

## In one minute

Four parts, configured on each machine, replace today's per-role tiers and Big/Small profiles. The host database is already per machine, so nothing syncs.

1. **Providers on this machine (hard limit).** Turn Claude and Codex on or off ("I have this subscription here"). Under each enabled provider, tick the models and efforts that may run.
2. **Providers per role (enforced).** Coordinators, implementer and tester each pick providers, not models. For example, Implementer → Claude.
3. **Review.** Tick the providers for the standard reviews. Each ticked provider gets one review, and they run in parallel after the tester passes. Free-text instructions add custom reviews, such as "Run reviso:style with Codex as a third review." The ticks define the standard reviews; they don't limit which providers a reviewer may use.
4. **Guide (prose, never parsed).** How to pick the model and effort within a provider by size and risk.

For every assignment, the coordinator picks an exact model and effort and gives a one-line reason. The host checks that the model is allowed on this machine and, for non-reviewer roles, that its provider is configured for the role. It records the choice and the reason, and the UI shows both. When the human picks a model, the human's choice wins.

### Example configuration (this machine, both subscriptions)

| Providers | Enabled | Allowed models · efforts |
|---|---|---|
| Claude | ✓ | opus · high, opus · medium, sonnet · high, sonnet · medium |
| Codex | ✓ | gpt-6.1-sol · high, gpt-6.1-sol · medium |

| Role | Providers |
|---|---|
| Main coordinator | Claude |
| Repository coordinator | Claude |
| Implementer | Claude |
| Tester | Claude |

| Review | |
|---|---|
| Standard reviews | ☑ Claude ☑ Codex: two reviews in parallel after the tester |
| Instructions | Run reviso:style with Codex as a third review. |

On a machine with only Codex, the human would disable Claude, set every role to Codex and tick only Codex for review.

### Example guide

```markdown
Pick the model by the size and risk of the work. Claude: small, contained change → Sonnet; large or cross-cutting → Opus. Codex: small → GPT 6.1 Sol at medium; large → GPT 6.1 Sol at high. Raise effort for migrations, data or security changes.
```

### Example assignments

```json
assign_ticket { "role": "implementer", "instruction": "Add the --json flag…",
  "profile": {"provider": "claude", "model": "sonnet", "effort": "high"},
  "reason": "Small, contained CLI flag; Sonnet is enough", … }

assign_ticket { "role": "reviewer", "focus": "Codex · reviso:style",
  "instruction": "Run reviso:style on this ticket's branch and report findings.",
  "profile": {"provider": "codex", "model": "gpt-6.1-sol", "effort": "medium"},
  "reason": "Custom review from the review instructions; routine style pass", … }
```

There are two kinds of rejection, and neither falls back to another model:
- An implementer assigned `codex · gpt-6.1-sol · high` fails with **"Codex is not configured for Implementer on this machine. Implementer uses: Claude."** This check doesn't apply to reviewers.
- `claude · opus · max` fails with **"claude · opus · max is not allowed on this machine. Allowed Claude models: opus · high, opus · medium, sonnet · high, sonnet · medium."**

## How the pieces fit together

```
Models page ─save─▶ host DB: model_selection row ─────────────────────▶ workspace_context
  Providers           { revision, providers, role_providers,            (coordinators: providers,
  Roles                 reviews { providers, instructions }, guide }      roles, reviews, guide)
  Review                         │                                             │
  Guide                          │                                             ▼
 Human pickers ◀── pre-fill ─────┤                       coordinator reads guide and reviews,
 (creation dialog,               │                       calls assign_ticket / create_repo_coordinator
  chat override)                 ▼                       with profile + reason
       │          model · effort allowed on this machine? ◀──────┘
       │          (non-reviewers) provider configured for role?
       │ (not checked:       │ yes                │ no
       │  human wins)        ▼                    ▼
       └─────────▶ session pinned; Selection     error to coordinator, model_rejected
                   {chooser, reason} shown       event, nothing created
                             │
                             ▼ at each turn start (coordinator-chosen sessions)
                   provider since disabled on this machine?
                             │ yes
                             ▼
                   agent Blocked, input kept queued; host sends a `blocked`
                   report to the parent on the agent's behalf (wakes it)
```

- **One record per machine.** Everything is in one `model_selection` row, saved together. `revision` increases on every save.
- **Delivery.** `workspace_context` gives both coordinator roles the enabled providers and their allowed models, the role providers, the review settings and the guide. It reads them fresh on every call. Workers get none of these.
- **Verification set.** After the tester reports passed, the repository coordinator starts the standard reviews (one per ticked provider) and any custom reviews from the instructions, all in parallel on the same ticket. Each review is its own reviewer with a distinct focus, such as "Claude", "Codex" or "Codex · reviso:style". The instruction text is passed to the reviewer it describes. The host enforces none of this; the coordinator skill tells the coordinator to do it.
- **Existing agents keep their model.** Unticking a model or changing a role's providers affects new assignments only. Disabling a provider holds that provider's coordinator-chosen agents at their next turn, because the subscription is no longer on this machine.

## UI

### (a) Models page

Today (`models_view.rs`, `docs/images/models.png`): four role cards, each with Default Claude/Codex toggles, Claude/Codex profile tabs and Big/Small model and effort rows.

New: four cards and one Save. Big/Small rows, profile tabs and per-role model dropdowns are removed.

```
┌ Workspace › Models on this machine ──────────────────────────────────────────── ✕ ┐
│ ┌ Providers ────────────────────────────────────────────────────────────────────┐ │
│ │ [●] Claude   CLI found                                                        │ │
│ │     Model            low   medium   high   xhigh   max                        │ │
│ │     Opus · latest    ☐      ☑       ☑      ☐       ☐                          │ │
│ │     Sonnet · latest  ☐      ☑       ☑      ☐       ☐                          │ │
│ │ [●] Codex    CLI found                                                        │ │
│ │     GPT 6.1 Sol      ☑      ☑       ☑      ☐       ☐                          │ │
│ └───────────────────────────────────────────────────────────────────────────────┘ │
│ ┌ Roles ────────────────────────────────────────────────────────────────────────┐ │
│ │ Main coordinator        [☑ Claude] [☐ Codex]                                  │ │
│ │ Repository coordinator  [☑ Claude] [☐ Codex]                                  │ │
│ │ Implementer             [☑ Claude] [☐ Codex]                                  │ │
│ │ Tester                  [☑ Claude] [☐ Codex]                                  │ │
│ └───────────────────────────────────────────────────────────────────────────────┘ │
│ ┌ Review ───────────────────────────────────────────────────────────────────────┐ │
│ │ Standard reviews  [☑ Claude] [☑ Codex]                                        │ │
│ │ One review per ticked provider, run in parallel after the tester.             │ │
│ │ Instructions (optional)                                                       │ │
│ │ ┌───────────────────────────────────────────────────────────────────────────┐ │ │
│ │ │ e.g. Run reviso:style with Codex as a third review.                       │ │ │
│ │ └───────────────────────────────────────────────────────────────────────────┘ │ │
│ └───────────────────────────────────────────────────────────────────────────────┘ │
│ ┌ Guide ── how to pick model and effort within a provider ──────────────────────┐ │
│ │ Claude: small, contained change → sonnet; large or cross-cutting → opus. …    │ │
│ └─────────────────────────────────────────────────────────────── 0.6 / 32 KB ──┘ │
├───────────────────────────────────────────────────────────────────────────────────┤
│ ⟳  Unsaved changes                                           Discard   [Save]     │
└───────────────────────────────────────────────────────────────────────────────────┘
```

- **Model rows** come from the existing catalog discovery plus saved values, as today's `choices`/`effort_choices` do.
- **CLI status** ("CLI found" or "CLI not found") comes from the same executable lookup as `ProviderCheck`, run when the model lists refresh. Sign-in is not checked; the first live turn verifies it.
- **Disabling a provider** greys out its models but keeps their ticks, and unticks it in every role and in Review. A role left with no provider shows an inline error that blocks Save.

### (b) Where the chosen model and reason appear

They appear in two places; the sidebar is unchanged.
1. **The agent's chat**, in a line above the composer's model pills: "Chosen by tidepool: Small, contained CLI flag; Sonnet is enough". It reads "Chosen by you" when the human picked the model.
2. **The Tickets panel**, where each ticket card lists its agents with their model and reason. The three reviewers appear as Claude, Codex and Codex · reviso:style.

### (c) The human's picker

The New agent and New repository team dialogs gain a Model field. It is pre-filled with the role's first configured provider, in Claude-then-Codex order, and that provider's first allowed model. For a reviewer, the first standard-review provider is used.

The menu lists the role's providers first, then "Other models". The human's choice wins, and nothing in the menu is blocked.

```
Model  [Claude · Opus · Medium                        ▾]
       ┌─────────────────────────────────────────────┐
       │ FOR IMPLEMENTER · CLAUDE                    │
       │ ✓ Claude · Opus · Medium                     │
       │   Claude · Sonnet · High                     │
       │ OTHER MODELS                                │
       │   Codex · GPT 6.1 Sol · High  not for role  │
       │   Claude · Haiku · Medium     not ticked    │
       └─────────────────────────────────────────────┘
```

### (d) Rejections

A rejection is a failed coordinator tool call. It appears as a failed `assign_ticket` step in that coordinator's work stream, with the host's message, and as a `model_rejected` entry in Events. No agent is created. There are two messages: wrong provider for the role, and model not allowed (see the examples above).

## Decisions

1. **Providers are the hard limit, per machine.** They state which subscriptions exist here and which models may run. They live in the host database, which is already per machine.
2. **Roles choose providers; coordinators choose models.** Coordinators, implementer and tester each have an enforced provider set. The coordinator picks the exact model and effort by the guide. Per-role default models are removed.
3. **Review is its own section, not a role entry.**
   - `reviews.providers` defines the standard reviews: one per ticked provider, run in parallel after the tester passes.
   - `reviews.instructions` adds custom reviews as prose. It is never parsed.
   - Reviewers are checked only against the machine's Providers allowlist, so custom reviews can use any enabled provider.
   - The host does not count or schedule reviews; the coordinator skill tells the coordinator to do it.
4. **The guide is advice only.** It is a single text field, never parsed, at most 32 KiB.
5. **Pre-fill is minimal.** It uses the role's first configured provider, in Claude-then-Codex order. Reviewers use the first standard-review provider, or the first enabled provider if none is ticked. Then it takes that provider's first allowed model in saved order. Human choices are never checked.
6. **A disabled provider holds its agents and wakes the parent.**
   - At each turn start, a coordinator-chosen session whose provider is now disabled is not started. The input stays queued.
   - The session is set Blocked with `last_error` "Codex is disabled on this machine".
   - The session is blocked first; then the host sends a `blocked` report to the parent on the agent's behalf, through the `report` path, with the same text. Its id is `report:‹agent›:turn-held:‹first held message sequence›`: bounded, and the same after a Retry, so a Retry that is held again sends no duplicate. A failed hold is logged and never stops other sessions from being scheduled; the report is re-sent on every later pass until it exists, so a refused or interrupted report still reaches the parent.
   - **Skip** drops the held input (the queued messages), as it drops held input elsewhere. It goes by a `held` flag saved on the runtime, not a fresh check, so it still drops the input if the provider was re-enabled meanwhile. The flag clears on Skip or when a turn starts.
   - Human-chosen sessions are never held. An agent below the main coordinator with no pinned model is held the same way, never given a substitute.
7. **The migration loosens model enforcement and tightens provider enforcement.**
   - **Models loosen.** Today's per-role allowed lists are unioned per provider, so a coordinator can give any role, for example, Sonnet, provided the role's provider matches.
   - **Providers per role tighten.** Today a coordinator can assign Codex implementers, testers and coordinators, because Codex profiles are on those roles' allowed lists. After migration, Implementer, Tester and both coordinator roles get **Claude only**, from their default provider. A Codex choice for them is rejected until the human ticks Codex for that role on the Roles card.
   - **Reviewers loosen.** They may use any enabled provider.

Token caps were considered and dropped. The host records no money, and token telemetry is not always complete.

## Data and API

```rust
pub struct ModelSelection {
    pub revision: u64,
    pub providers: BTreeMap<Provider, ProviderAccess>,
    pub role_providers: BTreeMap<Role, BTreeSet<Provider>>,  // coordinators, implementer, tester
    pub reviews: Reviews,
    pub guide: String,
}
pub struct ProviderAccess { pub enabled: bool, pub models: Vec<AllowedModel> }
pub struct AllowedModel { pub model: String, pub effort: String }
pub struct Reviews { pub providers: BTreeSet<Provider>, pub instructions: String }
pub struct Selection {                     // on SessionRuntime, beside the pinned `profile`
    pub chosen_by: Chooser,                // Coordinator { session_id } | Human | Default
    pub reason: String, pub revision: u64, pub at: i64,
}
```

- `ModelSelection::permits(role, profile)` returns `Err(NotAllowed { allowed })` when the provider is disabled or the pair is unticked. For roles other than Reviewer, it returns `Err(WrongProvider { allowed })` when the provider is not in `role_providers[role]`.
- `validate` requires:
  - at least one enabled provider, and at least one model for each enabled provider;
  - unique pairs within today's text bounds, at most 192 per provider (six legacy roles of 32 profiles, so the migration can never fail its own limit);
  - a non-empty set of enabled providers for each of the four roles;
  - review providers drawn from enabled ones (the set may be empty, meaning tester only);
  - instructions of at most 4 KiB and a guide of at most 32 KiB.
- Maintenance does no model work and gets no entry.

`RolePolicy` keeps only `role` and `turn_budget_minutes`.
- **Deleted:** `Complexity`, `RoutingMode`, `ProviderProfiles`, `RoleDefault`, `Route`, `RolePolicy::select`, and the `Route` and `SetRoleDefaults` commands.
- **Added:** `Command::SetModelSelection`.
- **Changed:** `Command::AssignTicket` and `Command::CreateSession` take `profile` instead of `provider`, and `Snapshot` gains `model_selection`.

### MCP arguments (model arguments only)

| Tool | Removed | Added (required) | Unchanged |
|---|---|---|---|
| `assign_ticket` | `provider`, `size`, `complexity`, optional `profile` | `profile` `{provider, model, effort}`, `reason` | `ticket_id`, `role`, `instruction`, `focus` |
| `create_repo_coordinator` | `provider`, `size` | `profile`, `reason` | `repository_id` |

The host checks a call in this order:
1. An existing agent or team is returned unchanged.
2. `size`, `complexity` and `provider` are rejected with a message naming the replacement.
3. The reason must be one line, non-empty and at most 200 characters.
4. `permits` must pass. Otherwise the call fails, a `model_rejected` event is logged and nothing is created.
5. On success, the host pins the profile, records the `Selection`, logs `model_selected`, and returns the session plus `profile` and `selection`. The session, its pinned runtime, its queued instruction, the ticket update and the events commit in one transaction, so an agent never exists without its model.

**Possible conflicts:**
- `provider` is a model argument, so removing it is in scope.
- `Host::assign_ticket` and the desktop creation commands change from `provider` to `profile`, which is a shared signature.
- Other work on `mcp.rs`, `live.rs::agent_tool` or the coordinator skills will hit textual merge conflicts.

### Skills and docs

The model paragraphs of `repository-coordinator.md` (v4) are replaced with:
- Read `model_selection` in `workspace_context`.
- For each assignment, pick a provider configured for the role, or any enabled provider for a reviewer. Then pick the exact model and effort by the guide and give a one-line reason.
- When the tester reports passed, start the verification set in parallel on the same ticket: one reviewer per standard-review provider (focus "Claude", "Codex"), plus one reviewer per custom review in the review instructions (focus such as "Codex · reviso:style"), passing that instruction text to it.
- Accept the ticket only after the tester has passed and every verification-set reviewer has reported, with each finding fixed or recorded. Accepting earlier would refuse the reviewers and remove the worktree they read.
- Never invent or substitute a model. If nothing allowed fits, report blocked.

`main-coordinator.md` gets the same first two points for `create_repo_coordinator`, and asks the human when nothing fits. The `mcp.rs` descriptions and the PRD's model-routing section are rewritten to match.

## Migration details

The migration is idempotent. It is recorded as `model-selection` in `policy_migrations` and runs after `migrate_opus_defaults`, which moves to private legacy types. Both migrations decode saved rows as `Option<LegacyRolePolicy>` and skip rows that are already slim. Everything happens in one transaction:

1. **Already recorded:** return without touching anything.
2. **Fresh store** (all rows slim): insert what step 4 produces from today's built-in policies, plus claude sonnet · high and · medium and codex gpt-6.1-sol · high so a new install can follow its starter guide. The guide is the fixed preamble.
3. **Pin sessions first.** Every unpinned session, including archived ones, is pinned to what today's `select(Standard, None, Some(provider))` returns. Its selection is recorded as `Default`, "Kept the model it used before". A session that fails today on a provider mismatch stays unpinned. An unpinned main coordinator's next turn takes its own provider's first allowed model as "Role default". Any other unpinned agent is held Blocked with "This agent has no pinned model…" and never given a substitute.
4. **Convert each field:**

| Legacy field | Destination |
|---|---|
| `default` and `allowed` (all six roles, any mode) | union grouped by provider → `providers[p].models`, every role's default first, then allowed lists in role order; `enabled` if it has any model |
| `default.provider` (coordinators, implementer, tester) | `role_providers[role] = {default.provider}` |
| reviewer `default.provider` | `reviews.providers = {default.provider}` |
| reviewer `provider_profiles` with both providers | `reviews.providers = {Claude, Codex}` |
| — | `reviews.instructions` = empty; the editor's placeholder shows the reviso:style example |
| `provider_profiles` big/small, per role and provider | guide sizing text: "Implementer · Claude: large work → opus · medium; small work → sonnet · high." |
| `small` / `standard` / `complex` | guide prose under the role |
| `mode: fixed` | guide prose ("Maintenance was fixed to gpt-6.1-sol · medium"); its other listed profiles are already on the allowlist and are not repeated, which keeps the worst-case guide within 32 KiB |
| maintenance row | its `allowed` joins the union; its default and tiers become prose |
| `turn_budget_minutes` (missing → `None`) | stays on the slim `RolePolicy` row |

5. **Starter guide**, in this order:
   1. The fixed preamble: "Pick the model by the size and risk of the work. Claude: small, contained change → Sonnet; large or cross-cutting → Opus. Codex: small → GPT 6.1 Sol at medium; large → GPT 6.1 Sol at high. Raise effort for migrations, data or security changes."
   2. "Previous settings (migrated ‹date›)": one subsection per role with its default and mode, its tiers and its per-provider Big/Small lines ("Claude: large work → opus · medium; small work → sonnet · high."). The two coordinators are merged when identical apart from their turn budgets.
   3. The tiers and fixed-mode notes.
6. Rewrite the `policies` rows slim, insert the record (`revision: 1`) and record the id.

The tests cover these shapes:
- the legacy fixed Codex shape (still the maintenance row);
- the built-in fixed Opus shape;
- custom fixed;
- automatic without Big/Small;
- automatic with Big/Small (the human's current data);
- rows without `turn_budget_minutes`.

Today's real data becomes:
- **Providers:** Claude {opus high/medium, sonnet high/medium} and Codex {gpt-6.1-sol high/medium/low, gpt-6-astra high}.
- **Roles:** Claude only for both coordinators, the implementer and the tester.
- **Reviews:** standard reviews {Claude, Codex}, and empty instructions.

## Risks / Trade-offs

- **Model enforcement loosens with the union.** This is the human's decision (Decision 7), and the sizing advice survives in the guide.
- **Provider enforcement tightens for the implementer, tester and coordinators.** Codex assignments that are allowed today are rejected until the human ticks Codex for the role. The rejection message names the role and its providers.
- **Review counts are not enforced by the host.** The coordinator skill and `workspace_context` carry them, and the Tickets panel shows which reviews actually ran.
- **Disabling a provider blocks its existing agents.** This is intentional: the subscription is gone from this machine. The parent is told, and the human can switch the agent's model from the chat.
- **Coordinators running with old skill text send `size`.** An explicit error names the replacement.
- **Downgrade is unsupported.** Older binaries cannot decode slim policy rows. Copy `workspace.sqlite3` before testing.
- **The capabilities live in the unarchived `native-workspace-foundation` change.** Archive or sync it before archiving this one.

## Open Questions

None.
