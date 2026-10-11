## 1. Suggestion and detection

- [x] 1.1 Add `workspace-core/src/suggestion.rs`: `ProviderStatus`, `Detection`, `Setup`, `starter_guide` and `suggest`. Test both, each provider alone (missing and signed out), neither, and that the plan changes nothing.
- [x] 1.2 Add `workspace-host/src/detection.rs`: `detect`, bounded to 10 seconds, with parsers for `claude auth status --json`, `codex login status` and the Codex ID token's plan claim. Test well-formed and malformed output, and a missing, unreadable and malformed `auth.json`, without calling a real CLI.

## 2. Host

- [x] 2.1 `migrate_model_selection` saves nothing on a store with no legacy policy; remove `legacy::fresh`; the migrated guide uses `starter_guide`.
- [x] 2.2 `create_project` refuses with the Models guidance while no setup is saved.
- [x] 2.3 `SetModelSelection` takes optional verifiers and checks both before saving either.
- [x] 2.4 `Host::open_set_up` for tests and benchmarks. Host tests: a fresh store saves nothing and refuses a project until a setup is saved, each one-provider suggestion saves exactly, a refused first save keeps the verifiers unsaved, and a saved store is byte-for-byte unchanged by reopening.

## 3. Models page

- [x] 3.1 Run detection with the catalog refresh and show installed, signed in and plan per provider.
- [x] 3.2 Offer the suggestion as an unsaved draft: banner, Suggested tags, one-line fixes, the Save bar's text, and Discard back to the suggestion. An untouched suggestion follows Refresh; an edited one is kept.
- [x] 3.3 Hold the Review card's verifiers as a draft until the first save sends them.
- [x] 3.4 `--demo-repository` creates its project only once a setup is saved.

## 4. Verify

- [x] 4.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace`, `python3 scripts/test_communications.py` and `openspec validate --all --strict`.
- [x] 4.2 Drive a debug build on fresh stores through `scripts/drive_desktop.py`: both signed in, Claude only, neither, an edit kept across Refresh, an untouched suggestion following Refresh, Save, and a saved store reopened unchanged.
