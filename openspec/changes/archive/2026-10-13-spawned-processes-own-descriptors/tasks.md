## 1. Cause

- [x] 1.1 Confirm against the Rust 1.97.0 standard library how macOS makes a pipe and starts a child, and reproduce the leak through `provider::configure`. Record both in `design.md`.

## 2. Fix

- [x] 2.1 Add `OwnDescriptors::own_descriptors_only` in `runtime.rs`: a hook in the child that marks every descriptor above stderr close-on-exec.
- [x] 2.2 Use it in `provider::configure`, `bounded_output`, the Codex and Claude probes in `runtime.rs` and the Claude context probe in `context.rs`.
- [x] 2.3 Test in `provider.rs` that eight providers started together 25 times, beside a Git query, each hold only their own descriptors. It fails on `main` and passes 250 times in a row with the fix.

## 3. Verify and archive

- [x] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked -p workspace-core -p workspace-host` and `python3 scripts/test_communications.py`, all passing.
- [x] 3.2 Run `openspec archive spawned-processes-own-descriptors`, then `openspec validate --all --strict`, which must pass.
