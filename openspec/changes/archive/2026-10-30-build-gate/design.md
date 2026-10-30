## Context

The host runs every agent's provider process at nice 10 with Cargo and nextest capped at half the cores. Agents run builds from their shells, often inside `nix develop`, and the human's shell profile may put `~/.cargo/bin` or a Nix profile first on PATH.

## Decisions

### Gate through build-tool hooks, not PATH
A shim directory first on PATH loses to `nix develop` and to profiles that prepend a toolchain; `nix develop --command` puts its store paths first. The variables Cargo and Go read to wrap their own tools are kept by both. So the host links `wiffletree-build-gate` to its own binary in `$TMPDIR/wiffletree-<hash of the data folder>/`, a folder without spaces (Cargo and Go split these values on them) that each restart reuses, and gives agents `RUSTC_WRAPPER`, `CARGO_TARGET_<host triple>_RUNNER` and `GOFLAGS=-toolexec=<gate>`. A user who already set one of them keeps theirs. The runner is needed because a cached `cargo test` re-run compiles nothing and would otherwise run every test binary ungated; it overrides a repository's own runner, which no repository on this machine sets. Go's `-exec` is not used: it turns off Go's test-result cache.

### What a build is
A build is the outermost `cargo` or `go` among a call's ancestors below the host's process whose subcommand builds: Cargo's build, check, clippy, test, nextest, bench, doc, rustc, rustdoc, install, run and fix, with their aliases, and Go's build, test, vet, install and run. Nested Cargo, such as `cargo clippy`'s, `cargo nextest`'s, a build script's or a test's, shares the outer build's slot and cannot deadlock against it. A wrapper that builds nothing itself, such as `cargo watch`, is skipped, so each `cargo test` it starts is its own build and frees its slot when it ends. The command line comes from `KERN_PROCARGS2` on macOS and `/proc/<pid>/cmdline` on Linux.

### A slot is held for the build's whole life
A build takes the slot at its first compile (a rustc or clippy-driver call naming a crate, except Cargo's target query, which reads `-` and only prints; or a Go tool other than its `-V` version query) or test binary (one the runner is handed from a `deps` folder), and keeps it until that process ends. Test binaries, doctests, `cargo doc`'s rustdoc and Go test binaries therefore run inside the slot. The slot records the process's start time and name, so an ended process, a reused id, or Cargo replaced by its app in `cargo run`, holds nothing; the app itself reaches the runner from outside `deps` and runs at once.

### `go run` holds its slot only while it builds
`go run` keeps Go running beside its app, so its slot would last as long as the app. Each of its compile and link calls instead takes a lease: a lock file the call keeps open across `exec`, which the system unlocks when the tool exits. The slot lasts while a lease is locked and for two seconds after the last one started, then frees.

### The gate becomes the program
After waiting, or straight away for any other call, the gate executes the program in its own place. Signals and exit statuses reach the program as without the gate, and `kill` of a background build stops it.

### Where the waiting line goes
Cargo caches rustc's standard error and replays it on later fresh builds, and Go caches a tool's output. A waiting compile therefore prints its line once on rustc's standard output, which Cargo shows but does not cache; a waiting test binary prints it on standard error; a Go compile waits silently.

### State
`<data>/build-gate/held/<pid>` names each build holding a slot with its start time and name, and marks a `go run`; `leases/<pid>/` holds a `go run`'s leases; `waiting/<pid>` files, by age, give the oldest waiting builds the free slots; `gate.lock` serialises decisions. The limit is read from `settings.json` on every check, so a change applies to builds already waiting. If the gate cannot keep its state, or the host cannot install it, builds run ungated.

## Risks and limits

- A repository's own `build.rustc-wrapper` such as sccache, and its own target runner, are overridden for agents, since Cargo prefers the variables; a wrapper or runner set in the host's environment is kept, and that part then goes ungated.
- A `go test` whose binary is cached but whose result is not, such as `-count=1` on an unchanged tree, makes no tool call and runs its tests without a slot.
- Go's first build with `-toolexec` may rebuild the standard library once.
