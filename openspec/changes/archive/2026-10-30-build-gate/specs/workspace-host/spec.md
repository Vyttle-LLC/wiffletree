## ADDED Requirements

### Requirement: Builds take turns across the host
The host SHALL let at most "Builds at once" builds compile or run tests at a time across every agent it runs, whatever the agent's project or role. It SHALL give each provider process `RUSTC_WRAPPER` and `GOFLAGS` with `-toolexec` and `-exec`, each naming the build gate, unless the host's own environment already sets that hook; it SHALL NOT set a Cargo target runner, so a repository's own runner keeps working. A call's build SHALL be the outermost Cargo, Go or Rust tool among its ancestors below the host, so nested Cargo shares its slot. A build SHALL take a slot at its first compile (a rustc or clippy-driver call naming a crate that does not read standard input or only print, or a Go tool other than its `-V` query) or Go test binary, waiting until one is free, and SHALL hold it until that process ends or becomes another program, so its test binaries, doctests and docs run inside it. `cargo run` SHALL leave no slot held by its running app, and neither SHALL `go run`. Every other call SHALL run at once. The gate SHALL end by executing the program in its own place, so signals reach the program. Waiting builds SHALL take free slots oldest first. A waiting Cargo build SHALL print "Waiting for a build slot (N in use)" once on rustc's standard output, which Cargo shows without replaying it from its cache, and a waiting Go test SHALL print it on standard error; a Go compile SHALL wait silently, since Go replays a tool's cached output. When the gate cannot keep its state, or the host cannot install it, builds SHALL run ungated. A repository's own `build.rustc-wrapper`, such as sccache, SHALL be overridden for agents; a build that compiles nothing, such as tests re-run on an unchanged tree, SHALL take no slot.

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
- **WHEN** a `go test` holds the only slot and its tests run for minutes after the last compile
- **THEN** another build waits until the `go test` ends

#### Scenario: A running app
- **WHEN** an agent's `cargo run` app is running, another build starts, and later the agent kills the `cargo run` process
- **THEN** the build does not wait, and the kill stops the app

#### Scenario: A repository's own runner
- **WHEN** a repository's `.cargo/config.toml` sets a target runner
- **THEN** `cargo run` and `cargo test` use it as without the gate

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
