## ADDED Requirements

### Requirement: Builds take turns across the host
The host SHALL let at most "Builds at once" builds compile or run tests at a time across every agent it runs, whatever the agent's project or role. It SHALL give each provider process `RUSTC_WRAPPER`, `CARGO_TARGET_<host triple>_RUNNER` and `GOFLAGS` with `-toolexec`, each naming the build gate, unless the host's own environment already sets that hook. The gate SHALL hold a call that compiles (a rustc or clippy-driver call naming a crate, or a Go tool other than its `-V` query) or runs a test or bench binary from a `deps` folder until the call's build holds a slot, and SHALL run every other call at once. A call's build SHALL be the outermost Cargo, Go or Rust tool among its ancestors below the host, so nested Cargo shares its slot. A build SHALL hold its slot while any of its calls runs and for one second after, and SHALL lose it when its process ends. Waiting builds SHALL take free slots oldest first, and a waiting build SHALL print "Waiting for a build slot (N in use)" to standard error once. When the gate cannot keep its state, the call SHALL run ungated.

#### Scenario: One build at once
- **WHEN** "Builds at once" is 1 and two agents in different projects each start a Cargo build
- **THEN** the second build's compiles wait until the first build's end, and its output says "Waiting for a build slot (1 in use)" once

#### Scenario: Two builds at once
- **WHEN** "Builds at once" is 2 and two builds start
- **THEN** both compile at the same time and neither says it waits

#### Scenario: Not a build
- **WHEN** a build holds the only slot and another agent runs `cargo fmt`, `cargo metadata` or `go version`
- **THEN** it runs at once

#### Scenario: Nix shell
- **WHEN** an agent runs `nix develop --command cargo test`, whose shell puts the Nix toolchain first on PATH
- **THEN** the build still goes through the gate

#### Scenario: A killed build
- **WHEN** the build holding the only slot is killed
- **THEN** the next waiting build starts

### Requirement: Builds at once is a host setting
"Builds at once" SHALL be a setting of the host, from 1 to 4, default 1, saved in this machine's `settings.json` and shown in Settings under Builds with one line explaining it. A value outside 1–4 SHALL be refused and the saved one SHALL stay. Builds already waiting SHALL use a new value at their next check.

#### Scenario: Saved
- **WHEN** the human chooses 3 and the host restarts
- **THEN** Settings shows 3 and at most three builds run at once
