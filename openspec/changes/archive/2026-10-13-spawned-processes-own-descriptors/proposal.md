## Why

When the host starts two providers at the same instant, one can inherit the write end of the other's input pipe. If the inheriting provider leaves a long-lived process, the other provider never sees the end of its prompt and its turn hangs. A restart test that queued two turns for one host start hung about once in 400 runs, and `lsof` on a failing run showed one agent's `sleep 300` holding the write end of the other provider's input pipe. `verify_ticket` starts four verifiers at once, and agents may now leave processes running until they are archived, so a leaked descriptor can be held for as long as an agent lives.

## What Changes

- **A started process holds only its own descriptors.** Every process the host starts is left only its standard input, output and error. Every other descriptor is marked close-on-exec in the child before it executes, so the child and whatever it starts hold no pipe the host made for something else.
- The provider's deliberate inheritance is unchanged: its three pipes, and the `WORKSPACE_SOCKET` and `WORKSPACE_TOKEN` environment its MCP helper uses.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `workspace-host`: a process the host starts holds only its own standard descriptors.

## Impact

- `crates/workspace-host/src/runtime.rs`: `OwnDescriptors::own_descriptors_only`, used by `bounded_output` (Git, `gh`, CLI detection) and the Codex and Claude probes.
- `crates/workspace-host/src/provider.rs`: `configure` uses it for every provider turn and compaction.
- `crates/workspace-host/src/context.rs`: the Claude context probe uses it.
- Tests: `provider.rs`, providers started together with a Git query beside them.
- No schema change or data migration.
