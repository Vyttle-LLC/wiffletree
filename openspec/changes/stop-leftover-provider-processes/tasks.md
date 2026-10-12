## 1. Provider processes

- [ ] 1.1 Record the leader's start time with each run's process group, in a new `provider_runs.process_started` column.
- [ ] 1.2 Stop a leftover group as `Process::stop` does, only while its leader is the recorded process. Test with real child processes: a confirmed group is stopped, and one with a different start time is not signalled.

## 2. Host

- [ ] 2.1 At startup, stop each unfinished run's leftover group and mark the run interrupted, recording when the process was stopped. Raise one inbox note for a group that could not be confirmed. Test both.
- [ ] 2.2 Hold a session while the group its note names still runs; start its turn once the group exits or the note is dismissed. Test through the real service that no turn starts until the process exits.
- [ ] 2.3 Add a blocking shutdown that returns once every running turn's process group is stopped, and record a run finished only after that. Call it when the desktop app quits and when the headless host exits. Test through the real service; the desktop call has no automated test.
- [ ] 2.4 Say in the retry note and the restart note when the interrupted turn's process was stopped at startup. Test the wording.

## 3. Verify and archive

- [ ] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace` twice and `python3 scripts/test_communications.py`, all passing.
- [ ] 3.2 Run `openspec archive stop-leftover-provider-processes`, then `openspec validate --all --strict`, which must pass.
