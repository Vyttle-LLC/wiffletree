## Why

A new install saves a built-in model setup silently, with Claude and Codex both enabled whether or not either CLI is installed or signed in. The first agent then fails on a provider the machine does not have, and the human never chose the setup they are running. The human approved a mockup in which the Models page suggests a setup from the CLIs it finds and saves nothing until they press Save.

## What Changes

- **Detection.** For each provider the app reads whether its CLI is installed and signed in, and the plan when known: `claude auth status --json`, and `codex login status` with the `chatgpt_plan_type` claim of the ID token in `~/.codex/auth.json`. Detection runs off the UI thread, is bounded to 10 seconds per CLI, and reads a missing, slow or failing CLI as not signed in. An unreadable plan is unknown, never an error, and no token contents are logged or shown.
- **No silent setup.** A fresh store saves no model selection. The Models page shows a suggested setup as an unsaved draft, with a banner and a Save that needs no edit first.
- **Suggestion.** Only providers installed and signed in are used. With both: today's roles, verifiers and starter guide, with Opus and Sonnet at high and medium and GPT 6.1 Sol at high and medium. With one: that provider takes every role, the verifiers are the tester and one reviewer on it, and the guide names only it. With neither: both are disabled and Save is disabled. The plan is shown and changes nothing.
- **No agent before the first save.** The host refuses to create a project, and so any agent, until a setup is saved, with a message pointing to the Models page. It never falls back to a built-in setup.
- **Refresh.** Refresh runs detection again. An untouched suggestion follows it; an edited one is kept.
- **Existing installs.** A store with a saved setup, including one migrated from legacy policies, is unchanged by the upgrade and by opening the Models page, apart from the richer status text.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `model-routing`: provider detection, the suggested first setup, no agent before the first save, and a migration that saves nothing on a fresh store.

## Impact

- `crates/workspace-core/src/suggestion.rs` (new): `ProviderStatus`, `Detection`, `Setup`, `suggest`, `starter_guide`. `selection.rs`: `ModelSelection::is_saved`. `lib.rs`: `SetModelSelection` may carry the verifiers.
- `crates/workspace-host/src/detection.rs` (new): `detect` and the status parsers. `policies.rs`: the migration saves nothing on a fresh store, `legacy::fresh` is removed, `Host::open_set_up` gives tests and benchmarks a saved setup. `lib.rs`: `create_project` refuses without a saved setup; `SetModelSelection` saves the verifiers sent with it.
- `crates/workspace-desktop/src/models_view.rs`, `models.rs`, `verifiers.rs`, `main.rs`: the suggested draft, banner, status text, fix hints and Save bar. `bridge.rs`: `--demo-repository` creates its project only once a setup is saved.
- Tests: `workspace-core/tests/suggestion.rs`, `detection.rs`, `tests/policies.rs`, `models.rs`; other host tests open their stores with `Host::open_set_up`. `scripts/test_communications.py` saves a setup first.
- No schema change. A fresh store has no `model_selection` row until the first save.
