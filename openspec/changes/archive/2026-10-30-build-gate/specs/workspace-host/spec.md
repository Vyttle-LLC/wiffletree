## ADDED Requirements

### Requirement: Builds take turns across the host
The host SHALL let at most "Builds at once" builds compile or run tests at a time across every agent it runs, whatever the agent's project or role. It SHALL give each provider process `RUSTC_WRAPPER`, `CARGO_TARGET_<host triple>_RUNNER` and `GOFLAGS` with `-toolexec`, each naming the build gate, unless the host's own environment already sets that hook. A call's build SHALL be the outermost Cargo or Go command among its ancestors below the host whose subcommand builds: any Cargo subcommand or alias except the wrappers `watch` and `make`, run by `cargo` or by the `cargo-<subcommand>` program Cargo replaces itself with, such as `cargo-nextest`; and Go's build, test, vet, install and run. Nested Cargo, under `cargo nextest` too, SHALL share its slot, and a wrapper SHALL hold none; with no such ancestor, the build SHALL be the call's parent. A build SHALL take a slot at its first compile (a rustc or clippy-driver call naming a crate that does not read standard input or only print, or a Go tool other than its `-V` query) or test binary (one Cargo or nextest runs from a `deps` folder), waiting until one is free, and SHALL hold it until that process ends or becomes another program, so its later test binaries and doctests run inside it, a cached re-run included; a `cargo doc` that only runs rustdoc SHALL take no slot. A `go run` SHALL hold its slot only while one of its compiles or its link runs and for two seconds after the last one started. The app `cargo run` starts SHALL run at once and hold no slot. Every other call SHALL run at once. The gate SHALL end by executing the program in its own place, so signals reach the program. Waiting builds SHALL take free slots oldest first. A waiting compile SHALL print "Waiting for a build slot (N in use)" once on rustc's standard output, which Cargo shows without replaying it from its cache, and a waiting test binary on standard error; a Go compile SHALL wait silently, since Go replays a tool's cached output. When the gate cannot keep its state, or the host cannot install it, builds SHALL run ungated. A repository's own `build.rustc-wrapper`, such as sccache, and its own target runner SHALL be overridden for agents. A `go test` whose binary is cached but whose result is not SHALL run its tests without a slot.

#### Scenario: One build at once
- **WHEN** "Builds at once" is 1 and two agents in different projects each start a Cargo build
- **THEN** the second build's compiles wait until the first build's end, and its output says "Waiting for a build slot (1 in use)" once

#### Scenario: Two builds at once
- **WHEN** "Builds at once" is 2 and two builds start
- **THEN** both compile at the same time and neither says it waits

#### Scenario: Not a build
- **WHEN** a build holds the only slot and another agent runs `cargo fmt`, `cargo metadata`, `cargo tree` on a fresh target folder or `go version`
- **THEN** it runs at once

#### Scenario: A Go test
- **WHEN** a `go test` that compiled holds the only slot and its tests run for minutes after the last compile
- **THEN** another build waits until the `go test` ends

#### Scenario: A running app
- **WHEN** an agent's `cargo run` app is running, another build starts, and later the agent kills the `cargo run` process
- **THEN** the build does not wait, and the kill stops the app

#### Scenario: A cached test re-run
- **WHEN** a build holds the only slot and another agent re-runs `cargo test` on a tree it already built, so nothing compiles
- **THEN** its test binaries wait for the slot, and its output says "Waiting for a build slot (1 in use)"

#### Scenario: Cargo inside a nextest test
- **WHEN** "Builds at once" is 1 and a test that `cargo nextest run` runs starts `cargo check`
- **THEN** the nested check shares the nextest run's slot and the test finishes

#### Scenario: A cargo watch
- **WHEN** an agent's `cargo watch` waits between the `cargo test` runs it starts
- **THEN** it holds no slot, and another agent's build starts

#### Scenario: A go run
- **WHEN** an agent's `go run` program is running after its build
- **THEN** another build starts without waiting for the program to end

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
