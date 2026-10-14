## 1. Freeze, look, stop

- [x] 1.1 `processes::stop_provider_group`: freeze the group with SIGSTOP and wait for its members to stop, call the look, then SIGTERM, SIGCONT, the grace period and SIGKILL. A guard continues the group if the look fails. `Process::stop` uses it with `processes::look`.
- [x] 1.2 `terminate` sends SIGCONT after SIGTERM, so `stop_group` stops a frozen group with SIGTERM.
- [x] 1.3 Tests in `processes.rs`: a Python provider stub forks a tool that calls `setsid` and the stop comes before any scan; a group left frozen whose leader handles SIGTERM; a look that panics. Each fails without its part of the fix.
- [x] 1.4 Remove the known gap from the archived design of `stop-agent-processes`.

## 2. A replaced verifier is stopped after the review starts

- [x] 2.1 `archive_in_change` and `stop_archived` in `lib.rs`; `verify_ticket` archives a replaced verifier inside its change and stops what it started once the change has committed.
- [x] 2.2 Test in `tests/archive.rs`: a review that fails after the replacement leaves the verifier unarchived, its process running and no notice, and a later review stops it. It fails with `change_archived` in the loop.
- [x] 2.3 Remove the uncovered case from the archived design of `stop-agent-processes`.

## 3. The desktop app's own processes

- [x] 3.1 Make `OwnDescriptors` public. Every command in `update.rs` goes through `command`, which applies the hook. The login shell and the test helpers are left alone; see the design.
- [x] 3.2 Test in `update.rs` that a command does not inherit an inheritable descriptor. It fails without the hook.
- [x] 3.3 Correct the rationale in the archived design of `spawned-processes-own-descriptors`.

## 4. The descriptor hook's fallback

- [x] 4.1 The hook's list size is a const parameter. A test with a list of four and eight inheritable descriptors shows the child holds only its own. It fails without the fallback loop.

## 5. The restart test

- [x] 5.1 Queue both turns before the headless host starts and find the tools from the record. 400 runs in a row passed.

## 6. Verify and archive

- [x] 6.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked -p workspace-core -p workspace-host`, `python3 scripts/test_communications.py` and `bash scripts/package_macos.sh`, all passing.
- [x] 6.2 Repeated runs: the `leftover_processes` binary 100 times and each new test 50 times, all passing.
- [x] 6.3 Run `openspec archive process-follow-ups`, then `openspec validate --all --strict`, which must pass.
