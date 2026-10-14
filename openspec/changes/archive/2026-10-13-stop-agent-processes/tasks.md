One implementation ticket. Tests that start real processes stop every one by its saved process id, also when they fail, use `sleep` and never CPU load, and use temporary directories, never the real store.

## 1. Facts

- [x] 1.1 Establish how macOS reuses a process id that is still a live group id, by test and by the kernel source, and write the finding in design.md.
- [x] 1.2 Find out whether Codex's tool commands share the provider's process group, and write the answer in design.md.
- [x] 1.3 Measure how long a Claude Code tool shell lives and what reading the process table costs, and choose the scan interval from them.

## 2. Processes

- [x] 2.1 `processes.rs`: read one process (id, parent, group, start time, command), list a group's members, and stop a group under the confirmation rule. Tests with real child processes: orphans of an exited leader are stopped; a group whose recorded members are gone and whose id belongs to another process is not signalled; one that ignores SIGTERM is killed.
- [x] 2.2 `processes.rs`: the shared watcher on a 4 ms clock. Tests with a watcher ticked by hand: a tool shell in its own group is recorded with its child; a child whose shell exited between two ticks is adopted from its group; a shell never listed leaves its child unrecorded; a finished command's group is reported gone; several trees share one listing.

## 3. Record

- [x] 3.1 `processes.sql` and `agent_processes.rs`: the `process_groups` and `process_group_members` tables, recording sightings, and the migration of `provider_runs.process_group` and `process_started`. Test the migration for an unfinished run, a run an older version finished without settling, and a settled run.
- [x] 3.2 Stop an agent's recorded groups, recording when and why, with the conversation notice and, for a group that could not be confirmed, the inbox note. Test the record, the notice and the note.

## 4. Host

- [x] 4.1 A run watches its provider and sends what the watcher saw before the run's end; the host records it. Correct the comment on `Process`.
- [x] 4.2 A normal turn end leaves the agent's processes running and adds the notice once; the agent's next turn takes it as input.
- [x] 4.3 Archive, by every path, stops the agent's processes before a worktree is removed; an agent archived mid-turn has them stopped when its turn ends.
- [x] 4.4 Pause, project Stop, `stop_agents`, a cancelled turn and a quit stop them before returning.
- [x] 4.5 Startup stops every group still recorded as running, then marks unfinished runs interrupted.
- [x] 4.6 The session hold and worktree removal read the record; only a provider's group holds a session.
- [x] 4.7 Tests in `tests/leftover_processes.rs` through the real `Service` for acceptance criteria 1 to 6, and one that pins the scan interval.

## 5. Instructions and docs

- [x] 5.1 `skills/communication.md` version 8: what you start keeps running until you are archived, and how to stop it.
- [x] 5.2 Update `docs/PRD.md` and `docs/DEVELOPMENT.md` where the behaviour they describe changes.

## 6. Verify

- [x] 6.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` twice and `python3 scripts/test_communications.py`, all passing.
- [x] 6.2 Run `openspec archive stop-agent-processes`, then `openspec validate --all --strict`, which must pass.
