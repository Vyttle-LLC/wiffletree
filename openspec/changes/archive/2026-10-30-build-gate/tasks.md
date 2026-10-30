## 1. Core

- [x] 1.1 `HostSettings::builds_at_once` (default 1, at most 4), `Command::SetBuildsAtOnce`, `ask::set_builds_at_once`.

## 2. Host

- [x] 2.1 `build_gate.rs`: the hooks, what counts as a build and a build's owner by its subcommand, slots held for a build's life oldest first, `go run` leases, executing the program in place, the waiting line, the limit from `settings.json`.
- [x] 2.2 `main.rs` runs the gate when called by its name and installs it for both host modes; `LowPriority` passes the hooks to providers.
- [x] 2.3 `set_builds_at_once`, saved on this machine.
- [x] 2.4 Remove `VERIFICATION_SLOTS` and the cycle queue; start cycles an earlier host queued; `verify_ticket` stops reporting `queued`.
- [x] 2.5 Tests: one build at once runs two builds one after the other and the second says it waits; two overlap; killing the gate kills the program; a killed build frees its slot; `cargo tree`, `metadata` and `fmt` run while the slot is held; a cached `cargo test` re-run waits through the runner; a `cargo watch`-style parent holds no slot between its builds; a build nested in a test under an external Cargo subcommand shares its slot; a real `go test` holds its slot until its tests end; a real `go run` frees its slot once its program runs; the setting persists and is bounded; every cycle starts at once; a stored queued cycle starts after an upgrade at HEAD, or ends blocked on a dirty worktree.

## 3. Desktop

- [x] 3.1 Settings → Builds: "Builds at once" 1–4 with one line explaining it.

## 4. Docs

- [x] 4.1 `docs/PRD.md`, `docs/DEVELOPMENT.md`, the `verify_ticket` tool description.
