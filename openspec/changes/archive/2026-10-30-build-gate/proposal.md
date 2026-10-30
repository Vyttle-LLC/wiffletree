## Why

Agents' builds took all of the human's CPU. Each build already gets half the cores (`CARGO_BUILD_JOBS`, `NEXTEST_TEST_THREADS`), so two at once fill the machine. The only bound was on review cycles: at most two held a verification slot, from start to end, between rounds included. That bounded the wrong thing. Implementers, reviewers and the PR keeper build too, while a reviewer-only re-check that builds nothing waited behind other projects' cycles.

## What Changes

- **A host-wide build gate.** At most "Builds at once" builds compile or run tests at a time across every agent on the machine, from any project and role. The rest wait their turn, oldest first, and print "Waiting for a build slot (N in use)" once, which shows in the agent's live work. Cargo (build, check, clippy, test, nextest, run, bench, doc) and Go (build, test, vet) are gated; calls that neither compile nor run tests, such as `cargo fmt`, `metadata`, `tree` and `--version`, pass straight through.
- **Hooks that survive PATH changes.** Agents reach the gate through `RUSTC_WRAPPER`, `CARGO_TARGET_<host triple>_RUNNER` and `GOFLAGS=-toolexec=…`, which shell profiles and `nix develop` keep, rather than a PATH shim they reorder. A hook the host's own environment already sets is left to the user.
- **"Builds at once" setting**, 1–4, default 1, in Settings under Builds, kept on this machine in `settings.json`.
- **No more verification slots.** Every `verify_ticket` cycle starts all its verifiers at once and never queues. A cycle an earlier host left queued starts at the next scheduler pass, at the worktree's HEAD, and ends blocked on uncommitted files as before.

## Capabilities

### Modified Capabilities
- `workspace-host`: the build gate and the "Builds at once" setting.
- `ticket-verification`: cycles start at once; stored queued cycles start on upgrade.
- `model-routing`: only a cycle queued by an earlier host resolves its verifiers when it starts.

## Impact

- `workspace-core`: `HostSettings::builds_at_once`, `Command::SetBuildsAtOnce`, `ask::set_builds_at_once`.
- `workspace-host`: `build_gate.rs`; `main.rs` runs the gate when called by its name and installs it; `runtime::LowPriority` passes the hooks to providers; `verification.rs` loses `VERIFICATION_SLOTS` and the queue; `verify_ticket` no longer reports `queued`.
- `workspace-desktop`: the Builds section of Settings.
- Docs: `docs/PRD.md`, `docs/DEVELOPMENT.md`, the `verify_ticket` tool description.
