## Why

Settings grew one screen at a time, and the same kinds of choice now live in several places. Four places pick a model: the Who runs what table, a Space's thread and job models, a Space's per-Mac allowed models, and a job recipe. Four carry free-text instructions: each skill's additions, a Space's instructions, a repository's `AGENTS.md`, and the coordinator notes. Nothing says which one won. A Space cannot ask for stricter review or publishing, a Space's instructions reach only its threads and jobs, and a choice such as "draw a mockup before a UI spec" can only be said as free text.

Michael settled the shape in a grill on October 31, 2026 (four decisions, recorded in `design.md`). He approved building ahead of a sign-off on the study in `ui/mockups.html`, and wants the study viewable before the pull requests merge.

## What Changes

- **One cascade.** Settings resolve Stock → Workspace → Space → Repository, per project. A project belongs to one Space, so a repository in two Spaces is never ambiguous. Each setting declares the layers that may set it, and the resolver returns each value with the layer it came from and the layers it overrode.
- **Safety settings only tighten.** A Space and a repository within it may make Publishing or Review depth stricter, never looser. A looser value is refused on save and ignored on read with a note. Allowed owners stay a This Mac permission.
- **Behaviors.** Common choices become named, typed Behaviors with stock defaults. Each renders one sentence into its role's skill, and the passage it controls leaves the stock text, so it is said once. The initial catalog has ten Behaviors across Coordinating, Implementing, Reviewing and Assisting.
- **Additions at every layer.** The Workspace keeps each skill's rules. A Space's instructions become its additions and now reach every role in the Space, not only threads and jobs. A repository within a Space gets its own additions, for its tickets' agents only. Full rewrite stays, on the Workspace layer only.
- **Storage.** The Workspace's Behaviors are `settings/behaviors.json` in the shared folder, synced like the other shared settings. A Space's are `behaviors:` in its `space.yaml` beside its additions, which stay stored under their old key `instructions:` so older hosts keep reading them, and its repositories' are under `repository_settings:`. Unknown keys survive every write.
- **Enforcement.** "Publishing: always ask first" makes `publish_ticket` ask the human even in an allowed owner's repository. "Review depth: full review for every ticket" makes `verify_ticket` refuse re-check levels.
- **Provenance everywhere.** Host commands read the catalog, read and set one layer's values (the human's client only), and read a project's or ticket's effective settings. A coordinator's `workspace_context` carries its resolved Behaviors with provenance.
- **Later stages** (this change's tasks 2–4): Space row overrides of the Who runs what table and of review caps, retiring `SpaceLocal.models`; the Behaviors page replacing Skills; the Models, Spaces and Settings screens, and Effective settings.

## Capabilities

### New Capabilities
- `settings-cascade`: the layers, the resolver and its provenance, the safety rule, where each layer is stored, the Behaviors catalog, how instructions compose, and the host commands.

### Modified Capabilities
- `skills`: instructions now compose core, skill text, resolved Behaviors, then Workspace, Space and Repository additions; Behavior passages left the stock texts.
- `spaces`: a Space's instructions are its additions and reach every role; a Space carries Behaviors and per-repository settings.
- `ticket-publishing`: the Publishing Behavior can make every publish ask.
- `ticket-verification`: the Review depth Behavior can require full reviews.

## Impact

- `workspace-core`: new `cascade` module (`Layer`, `Resolved`, `Setting`, `resolve`, `check`, `Behavior`, `Scope`, `LayerValues`, `EffectiveSettings`); `Space.instructions` becomes `Space.additions`, still stored as `instructions` with `behaviors` and `repository_settings`; `PublishRequest.always_ask` and `PublishAsk::AlwaysAsk`; four new commands.
- `workspace-host`: new `behaviors` module (catalog, resolution, composition, layer reads and writes); `settings/behaviors.json`; `space.yaml` keys; `session_instructions` for every turn; `publish_ticket` and `verify_ticket` enforcement; `workspace_context`.
- Stock skills: Coordinating v6, Implementing v3, Reviewing v3, Assisting v2.
- `docs/host-protocol.md`: the four commands; `create_space` takes `additions`.
- No GPUI screen changes in stage 1 beyond the Space page's field name.
