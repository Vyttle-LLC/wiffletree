# Scan fixtures

Every value here is invented. These files are the one fixture set for the
entry contract: Wiffletree's log writer tests in `memory.rs` check every one of
them. Change them only together with the contract.

- `brain/` is a small brain repository: the base commit of every test. Its
  `wiffletree-brain.json` and `brain-gate-allowlist.toml` are the checks' config.
- `entries/good/` holds entries the contract accepts, and `entries/bad/` entries
  it refuses. `entries/bad/problems.json` lists each bad entry and what breaks it.

Each entry is checked at
`second-brain/logs/sms-service/2026/10/2026-10-29-01jbx4m2k8q6v9z3n5r7t0w2yb.md`.
