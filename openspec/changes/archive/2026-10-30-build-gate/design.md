## Context

The host runs every agent's provider process at nice 10 with Cargo and nextest capped at half the cores. Agents run builds from their shells, often inside `nix develop`, and the human's shell profile may put `~/.cargo/bin` or a Nix profile first on PATH.

## Decisions

### Gate through build-tool hooks, not PATH
A shim directory first on PATH loses to `nix develop` and to profiles that prepend a toolchain; `nix develop --command` puts its store paths first. The variables Cargo and Go read to wrap their own tools are kept by both. So the host links `wiffletree-build-gate` to its own binary in `$TMPDIR/wiffletree-<hash of the data folder>/`, a folder without spaces (Go splits its flags on them) that each restart reuses, and gives agents `RUSTC_WRAPPER` and `GOFLAGS=-toolexec=<gate> -exec=<gate>`. A user who already set one of them keeps theirs. No Cargo target runner is set: it would override a repository's own runner and put the gate between `cargo run` and its app.

### A slot is held for the build's whole life
A build is the outermost Cargo, Go or Rust tool among a call's ancestors below the host's process, so the nested Cargo of `cargo clippy`, `cargo nextest`, a build script or a test shares the outer build's slot and cannot deadlock against it. It takes the slot at its first compile: a rustc or clippy-driver call naming a crate, except Cargo's target query, which reads `-` and only prints; or a Go tool other than its `-V` version query; or a Go test binary, which `-exec` hands to the gate. It keeps the slot until that process ends. Test binaries, doctests, `cargo doc`'s rustdoc and Go tests therefore run inside the slot without hooks of their own, and no grace period is needed between compiles. The slot records the process's start time and name, so an ended process, a reused id, or Cargo replaced by its app in `cargo run`, holds nothing. `go run` keeps Go running beside its app, so when `-exec` hands the gate a binary from Go's `exe` folder, the gate gives up that `go` process's slot first.

### The gate becomes the program
After waiting, or straight away for any other call, the gate executes the program in its own place. Signals and exit statuses reach the program as without the gate, and `kill` of a background build stops it.

### Where the waiting line goes
Cargo caches rustc's standard error and replays it on later fresh builds, and Go caches a tool's output. A waiting Cargo build therefore prints its line once on rustc's standard output, which Cargo shows but does not cache; a Go test binary prints it on standard error; a Go compile waits silently.

### State
`<data>/build-gate/held/<pid>` names each build holding a slot with its start time and name; `waiting/<pid>` files, by age, give the oldest waiting builds the free slots; `gate.lock` serialises decisions. The limit is read from `settings.json` on every check, so a change applies to builds already waiting. If the gate cannot keep its state, or the host cannot install it, builds run ungated.

## Risks and limits

- A repository's own `build.rustc-wrapper` such as sccache is overridden for agents, since Cargo prefers the variable; a wrapper set in the host's environment is kept, and rustc then goes ungated.
- A build that compiles nothing, such as tests re-run on an unchanged tree or `cargo doc` with every dependency already checked, takes no slot.
- A long-lived Cargo subcommand such as `cargo watch` holds its slot while it runs.
- Go's first build with `-toolexec` rebuilds the standard library once, since the hook is part of Go's cache key.
