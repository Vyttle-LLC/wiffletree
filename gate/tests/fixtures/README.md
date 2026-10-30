# Brain gate fixtures

Every value here is invented. These files are the one fixture set for the
entry contract: the gate's tests use them, and so do Wiffletree's log renderer
tests (`brain-log-capture`). Change them only together with the contract.

- `brain/` is a small brain repository: the base commit of every test PR. Its
  `wiffletree-brain.json` and `brain-gate-allowlist.toml` are the gate config.
- `entries/good/` holds entries that `log-schema` accepts, and
  `entries/bad/` entries it rejects. `entries/bad/problems.json` lists, for each
  bad entry, the text that `log-schema`'s output for it must contain.

Each entry is checked at
`second-brain/logs/sms-service/2026/10/2026-10-29-01jbx4m2k8q6v9z3n5r7t0w2yb.md`.
