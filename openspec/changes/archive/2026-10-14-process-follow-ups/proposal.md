## Why

The reviews of PR #62 (stop everything an agent started) and PR #59 (each process the host starts keeps only its own descriptors) left five small items. Two are gaps in what the host stops, one is a place the descriptor fix did not reach, and two are tests.

## What Changes

- **A provider's group is frozen before the stop's last look.** A tool the provider forked into a session of its own just before a cancel was orphaned by the stop before any scan listed it, so it was never recorded and survived the Stop. The turn now freezes its provider's group with SIGSTOP, has the watcher look while it is frozen, and only then sends SIGTERM. Every SIGTERM the host sends is followed by SIGCONT, so a frozen process acts on it, and a look that fails continues the group.
- **A replaced verifier is stopped once its review has started.** `verify_ticket` archived a verifier it replaced, and stopped what that verifier started, inside one savepoint. When a later step of the same call failed, the rollback erased the stop's record and its notice, though the processes stayed stopped and the verifier stayed active. The stop now waits until the review has started, so a review that fails to start stops nothing.
- **The desktop app's update commands own their descriptors.** The host runs inside the desktop process, so `curl`, `ditto`, `codesign` and the shell that swaps the bundle get the same close-on-exec hook as the host's own processes.
- **Tests.** The descriptor hook's fallback, when its list may be cut short, has a test. The restart test starts its two turns together again.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `workspace-host`: a frozen provider group and SIGCONT after SIGTERM; the replaced verifier's stop after the review starts; the desktop app's update commands hold only their own descriptors.

## Impact

- `crates/workspace-host/src/processes.rs`: `Frozen`, `stop_provider_group`, `look`; `terminate` sends SIGCONT after SIGTERM.
- `crates/workspace-host/src/provider.rs`: `Process::stop` uses `stop_provider_group`.
- `crates/workspace-host/src/lib.rs`, `verification.rs`: `archive_in_change` and `stop_archived`; `verify_ticket` stops replaced verifiers after its change commits.
- `crates/workspace-host/src/runtime.rs`: `OwnDescriptors` is public; the hook's list size is a parameter, so a test can force the fallback.
- `crates/workspace-desktop/src/update.rs`: every command goes through `command`, which applies the hook.
- Tests: `processes.rs`, `runtime.rs`, `update.rs`, `tests/archive.rs`, `tests/leftover_processes.rs`.
- Docs: `docs/DEVELOPMENT.md`; the archived designs of `stop-agent-processes` and `spawned-processes-own-descriptors`.
- No schema change or data migration.
