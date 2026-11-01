## Context

Decided in the grill on 2026-10-31; the study is `ui/mockups.html`. Michael approved building without a separate mockup sign-off, and wants the study viewable before the pull requests merge.

Where every setting lived at the start (origin/main 835ae6c8):

| Setting | Stored in | Scope | Overlap or problem |
|---|---|---|---|
| Appearance, layout, finding Macs, paired computers | app | This Mac (client) | none |
| Workspace folder, builds at once, remote access | host `settings.json` | This Mac | none |
| Settings repository chosen | host `settings.json` | This Mac | none |
| Allowed owners (publish without asking) | host `settings.json` | This Mac | reads like policy, is a permission |
| Repository path, base, checks command, protected branches | host DB | This Mac, per repo | none; repository facts |
| Who runs what table (every row) | host DB | This Mac | **Models #1** |
| Switch threshold, model guide | `settings/models.json`, `model-guide.md` | Workspace | none |
| Review settings: verifiers, levels, round and cycle caps | `settings/review.json` | Workspace | no Space can require stricter review |
| Check-in thresholds | `settings/check-ins.json` | Workspace | none |
| PR keeper triggers, quiet period, review level | `settings/pr-keeper.json` | Workspace | keeper *model* is per Mac |
| Assistant name, router threshold | `settings/assistant.json` | Workspace | none |
| Skills: stock text + additions or rewrite, per role | `skills/<role>/` | Workspace | **Instructions #1**; global only |
| Space description, instructions | `spaces/<s>/space.yaml` | Space | **Instructions #2**; reached threads and jobs only |
| Space thread and job default models | `space.yaml` `models` | Space | **Models #2**; bypasses the table |
| Space allowed models | host DB `SpaceLocal.models` | This Mac, per Space | **Models #3** |
| Job recipe model | recipe file | per job | **Models #4** (fine: most specific) |
| Space sources; recipe sources | `space.yaml`, recipe | Space, per job | fine |
| Space repositories | `space.yaml` | Space | fine |
| Space brain; brain path | `space.yaml`; host DB | Space; This Mac | fine |
| Brain compiler host, compile time, gate | `wiffletree-brain.json` | per brain | correct, but invisible from the Space |
| Tracker prefix, states, labels | `wiffletree-issues.yaml` | per storage repo | fine |
| Repository instructions | `AGENTS.md` / `CLAUDE.md` | per repo | **Instructions #3**; read by the provider |
| Coordinator notes | `model_selection.guide` | Workspace | **Instructions #4** |

## Goals / Non-Goals

**Goals:** one cascade with provenance; Behaviors for common choices; additions at every layer; Space additions reaching every role; safety settings that only tighten; one model table (stage 2); screens that say where each value came from (stages 3–4).

**Non-Goals:** moving the Who runs what table into the shared folder; repository-layer model overrides; settings a project overrides on its own (a project is short-lived; use its Space); Behaviors for the PR keeper, whose stock text is two procedures.

## Decisions

### The layers

| Layer | Who it describes | Stored in | Shared with |
|---|---|---|---|
| **Stock** | Wiffletree's defaults | code | everyone |
| **Workspace** | the human, on every Mac | the shared folder | the human's Macs |
| **Space** | one Space, such as Sage | `spaces/<s>/space.yaml` | the Space's members |
| **Repository** | one repository *within a Space* | the same `space.yaml`, under `repository_settings` | the Space's members |
| **This Mac** | this machine | host DB and `settings.json` | nobody; never cascades |

- **Resolution is per agent start.** An agent in a project resolves Workspace → its project's Space → that Space's entry for the ticket's repository. A coordinator, thread or job has no single repository and stops at Space. Ticket agents are the worker roles: implementer, maintenance, tester, reviewer and PR keeper.
- **Repository overrides live in the Space.** Repositories are a per-Mac record, so an override stored on it would not travel; stored in the Space, it travels with the Space and is naturally per project. A repository in two Spaces may differ between them. A Space may set values only for a repository it lists.
- **`repository_settings`, not `repositories`.** `space.yaml` already has `repositories:`, the list of `owner/name` the Space covers, which older hosts read as a list. Turning it into a map would make every older host fail to read the Space, so the per-repository settings get their own key.
- **Safety settings only tighten.** A Space is writable by whoever can push its storage repository. A Space or repository layer may make Publishing or Review depth stricter than the value it inherits, never looser. A looser value is refused on save, comparing with what this Mac inherits, and ignored on read with a note, so a member whose Workspace is stricter keeps it. The Workspace is the human's own layer and may loosen what it set itself. Allowed owners stay This Mac.
- **Provenance is data.** `cascade::resolve` returns each value with the layer it came from, the earlier layers it overrode (Stock first) and a note for each layer's value it ignored. It is generic over a `Setting` (stock value, allowed layers, refusal), so stage 2's model rows use the same resolver.
- **Unknown keys survive.** Every new key is written beside the existing `extra` maps. A layer's Behavior choices are a map of id to value; ids this release does not know are kept on save.

### Behaviors

A Behavior is a named, typed choice: an id, a name, the skill it renders into, the layers that may set it, its choices (stock first), each with a label and one sentence, and whether it only tightens. The host renders the resolved sentences into the role's instructions. An empty sentence says nothing. The catalog lives in the host beside the stock skills and is versioned with them: changing a sentence bumps its skill's stock version, and a passage a Behavior controls leaves the stock text, so each sentence is said once. A test holds that no choice's sentence is still in its skill's stock text.

| Behavior | Id | Role | Choices (stock first) | Layers |
|---|---|---|---|---|
| Mockup before a UI spec | `mockup_before_ui_spec` | Coordinator | on / off | W, S, R |
| Fetch before naming a landing order | `fetch_before_landing_order` | Coordinator | on / off | W, S, R |
| Confirm a relayed answer moved the child | `confirm_relayed_answer` | Coordinator | on / off | W, S |
| Status table | `status_table` | Coordinator | on finish or when asked / with every update | W, S |
| Review depth | `review_depth` | Coordinator | proportional to risk / full review for every ticket (tightens) | W, S, R |
| Publishing | `publishing` | Coordinator | as allowed owners say / always ask first (tightens) | W, S, R |
| Commit work in progress at milestones | `wip_commits` | Implementer | on / off | W, S, R |
| Optional findings per round | `optional_findings` | Reviewer | 5 / 3 / 10 | W, S, R |
| Answer length | `answer_length` | Assistant | brief / thorough | W, S |
| Drafts | `drafts` | Assistant | Markdown, HTML for visuals / Markdown only | W, S |

- **The first three are on in stock.** The grill's note took them for Michael's own additions, off in stock. They are in fact sentences of the stock Coordinating text since the first skills release, so every coordinator says them today. Stock "on" keeps every coordinator's instructions as they were; Michael, or anyone, can turn them off at any layer.
- **Coordinator Behaviors at the Repository layer.** A coordinator stops at its Space, so a repository's value for a coordinator Behavior does not change its instructions. It does decide Publishing and Review depth for that repository's tickets, which the host enforces, and the coordinator's `workspace_context` lists every repository whose Behaviors differ from the Space's.
- **Enforcement.** "Publishing: always ask first" is recorded on the publish request when `publish_ticket` starts it (`PublishRequest.always_ask`), so the host and the Inbox card decide alike: each step asks, as in a repository whose owner is not allowed, and the card names the Behavior. The PR keeper's own pushes keep their rules. "Review depth: full review for every ticket" makes `verify_ticket` refuse a re-check level; the PR keeper's re-check of its own rebase is not a `verify_ticket` call and keeps its rule.

### How an agent's instructions compose

1. The communication contract and the role's core (fixed; no layer changes them).
2. The role's stock text, or the Workspace rewrite (Advanced).
3. The resolved Behaviors, one sentence each, in catalog order.
4. Additions: the Workspace's (the skill's own rules, under their heading; none with a rewrite), then the Space's, then the Repository's, each under a heading of Wiffletree's naming where it came from and saying it cannot widen the agent's authority.
5. The repository's own `AGENTS.md`, which the provider reads as today.

Space additions reach every role in the Space, not only threads and jobs. A thread's note about the issue it was opened on comes before its Space's additions, as before. Every turn, compaction and context measurement composes through one function, `Host::session_instructions`.

### Storage

- **Workspace:** `settings/behaviors.json`, `{"behaviors": {"<id>": "<choice>"}}`, written with the other shared files' rules (sorted keys, unknown fields kept), synced, paused and compared like them. Sync refuses a file with a choice a Behavior does not offer.
- **Space:** `behaviors:` at the top of `space.yaml`; the additions stay under their old key, `instructions:`. The code and the screens call them additions, but the file keeps the one key every release reads, so a teammate on an older host keeps getting a shared Space's additions, and a save from either side never leaves two keys that could diverge. An existing file reads unchanged.
- **Repository:** `repository_settings: {"<owner/name>": {behaviors, instructions}}` in the same file, the additions named as the Space's are; an entry left empty is removed.

### Host commands

`behavior_catalog`, `layer_values` and `effective_settings` read; `set_layer_values` writes and is the human's client only, as Space edits are. No agent tool writes any layer. The Workspace layer's additions stay each skill's rules (`save_skill_rules`). `create_space` takes `additions`, and still accepts an older client's `instructions`.

### Models (stage 2)

- The Who runs what table stays per Mac (subscriptions differ per Mac). It is the base layer.
- A Space may override any row: `space.yaml` `models.rows`, the same row shape as the table. `models.threads` and `models.jobs` are read as overrides of the Assistant and Job rows.
- Resolution: recipe model (jobs only) → Space row → this Mac's row. A choice whose provider is not signed in here falls through with the existing kind of note.
- `SpaceLocal.models` is retired; a one-time note on the Spaces page says so.
- A Space may override the round cap, the full-review cap, and which review levels the coordinator may use; a Space may not drop a verifier with an open finding.
- `workspace_context` shows the coordinator the table as resolved for its project's Space, with "from Sage" provenance on overridden rows.

### Screens (stages 3–4; see `ui/mockups.html`)

1. **Behaviors** (replaces Skills): a scope bar `Workspace · Space ▾ · Repository ▾`; per role, each Behavior with its value, a provenance chip and Reset; additions for the scope; an Advanced disclosure holding today's rewrite, history and merges (Workspace only).
2. **Models**: the same scope bar (`This Mac · Space ▾`); overridden rows marked, Reset per row.
3. **Space page**: models and instructions leave it; it links "Behaviors for Sage" and "Models for Sage", and shows its brain's compile time and gate read-only.
4. **Settings window**: two groups, "Shared with your Macs" and "This Mac"; repository facts, allowed owners, signed-in providers and paths under This Mac.
5. **Effective settings**: from a project's header, a read-only list of what its agents get and where each value came from.

## Risks / Trade-offs

- **Mixed host versions in one Space.** An older host keeps `behaviors:` and `repository_settings:` through `extra` and round-trips them, but does not act on them: its agents get no Behaviors from the Space and no repository additions. It keeps reading the Space's additions, since they stay under `instructions:`, though it gives them only to threads and jobs. Members update together in practice; the Space page may note it in stage 4.
- **A rewrite based on an older stock version** still holds a passage a Behavior now says, so it is said twice until the rewrite merges the new stock version, which the merge machinery offers or does on its own.
- **Safety refusals depend on this Mac's Workspace.** A Space value saved here may be ignored on a Mac whose Workspace is stricter; the note says so there.
