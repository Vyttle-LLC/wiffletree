## Context

The host runs every agent's provider process at nice 10 with Cargo and nextest capped at half the cores. Agents run builds from their shells, often inside `nix develop`, and the human's shell profile may put `~/.cargo/bin` or a Nix profile first on PATH.

## Decisions

### Gate through build-tool hooks, not PATH
A shim directory first on PATH loses to `nix develop` and to profiles that prepend a toolchain; `nix develop --command` puts its store paths first. The variables Cargo and Go read to wrap their own tools are kept by both. So the host links `wiffletree-build-gate` to its own binary in a folder without spaces (Cargo splits a runner on whitespace) and gives agents `RUSTC_WRAPPER`, `CARGO_TARGET_<host triple>_RUNNER` and `GOFLAGS=-toolexec=<gate>`. A user who already set one of them keeps theirs, and that tool is not gated. A wrapper set only in a repository's Cargo config is overridden by the variable, which Cargo prefers.

### What counts as a build
A rustc or clippy-driver call with `--crate-name`, a binary from a `deps` folder (test and bench binaries, as Cargo and nextest run them), or a Go tool other than its `-V` version query. `cargo run` binaries live outside `deps`, so a running app or server holds no slot once compiled.

### A slot belongs to a build, not a call
Cargo starts many rustc processes per build, and `cargo clippy`, `cargo nextest` and tests can start a nested Cargo. A call's build is the outermost Cargo, Go or Rust tool among its ancestors below the host's process, so nested work shares the outer build's slot and cannot deadlock against it. A build holds its slot while any of its calls runs and for one second after, so the gaps between compiles do not hand the slot away, and loses it at once when its process is gone.

### State in lock files
`<data>/build-gate/` holds one folder per build with a lock file per running call. The system unlocks a file when its holder dies, so a killed build never keeps a slot. A `gate.lock` serialises decisions, and `waiting/<build>` files, by age, give the oldest waiting builds the free slots. The limit is read from `settings.json` on every check, so a change applies to builds already waiting.

### The gate never blocks a build
If the gate cannot keep its state, the call runs ungated. A waiting call whose build has gone away exits.

## Risks

- Go's first build after the hook appears rebuilds the standard library once, since `-toolexec` is part of Go's cache key.
- A build script that compiles C code itself is not a gated call; it runs inside its build's slot or between calls.
