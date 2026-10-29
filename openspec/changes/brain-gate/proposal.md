## Why

The brain's PR gate is what makes auto-merge safe. Its checks are: a secret scan, a sensitive-information scan for child PII and credentials, markdownlint, wikilinks, the log entry schema and size cap, path ownership, and the gate's own fixture tests. Today it lives inside one brain repository: `sagehaven-docs` PR #1, with `scripts/gate/`, `.github/workflows/brain-gate.yml` and the README's contract.

Every brain Wiffletree manages needs the same gate, and Wiffletree's own log writer (`brain-log-capture`) must produce exactly the entry contract the gate enforces. Copying the scripts into each brain would let the gate and the contract drift between brains and away from Wiffletree.

Michael decided on October 29, 2026 that the gate belongs in Wiffletree. Wiffletree owns the checks, the entry contract, the fixture tests and their versions. It ships them as a reusable GitHub Action from this public repository, so the gate can move to its own repository later. A brain repository keeps only a thin caller pinned to a version, and its own organization-specific config.

## What Changes

- **The gate moves into Wiffletree** as a composite GitHub Action under `gate/`, with:
  - the seven checks: `secret-scan`, `sensitive-info`, `markdown-lint`, `wikilinks`, `log-schema`, `path-ownership` and `gate-tests`;
  - their default rule configs;
  - the entry contract;
  - the fixture tests.

  Every check behaves exactly as in `sagehaven-docs` PR #1: deterministic, no model calls, no matched value ever echoed, and suppression only by an explicit inline marker, an allow-list entry with a reason, or gitleaks' own mechanisms.
- **Versioned releases.** The gate is tagged `brain-gate-vX.Y.Z`, with a moving `brain-gate-vX` major tag. Any change that rejects input the previous version accepted, the entry contract included, is a new major version. Wiffletree's CI runs the fixture tests on every change to `gate/`.
- **One contract, one fixture set.** Wiffletree's log renderer (`brain-log-capture`) is tested against the same fixtures in this repository, so the writer and the gate cannot disagree.
- **A thin caller in each brain repository**: `.github/workflows/brain-gate.yml`, with one job per check, each using the action pinned to a release by commit SHA. The brain repository keeps its own config:
  - the allowed email domains, the size cap and the legacy-log flag, under `gate` in `wiffletree-brain.json`;
  - the value and path allow-list, in `brain-gate-allowlist.toml`;
  - its own gitleaks allowances, if any;
  - `CODEOWNERS`, covering the caller, the config and the allow-list.

  Organization-specific values never go into Wiffletree. An optional summary job, such as Sage's check aggregator comment, stays in the caller and is not part of the gate.
- **Set up and update through a PR.** `brain-registry-sync`'s Set up installs the caller, the `gate` config and an empty allow-list in one PR. When a newer gate version is out, the Brains page offers Update gate, which opens a PR that changes only the pin.
- **No code moves in this proposal.** Moving `scripts/gate/` from `sagehaven-docs` into Wiffletree, publishing the first release and replacing PR #1's in-repository scripts with the caller are this change's tasks.

## Capabilities

### New Capabilities
- `brain-gate`: the gate action and its seven checks, the entry contract, suppression, versioning and fixtures, and the caller and config each brain repository carries.

### Modified Capabilities
<!-- None in openspec/specs. brain-registry-sync (in this same proposal) gains the gate's install and update in its Set up flow. -->

## Impact

- New `gate/` in this repository:
  - `action.yml` (composite, input `check`);
  - `checks/` (the Python checks and `common.py` from PR #1);
  - `config/` (default `gitleaks.toml` and `markdownlint-cli2.jsonc`);
  - `tests/` (the fixture tests);
  - `README.md` (the contract, the rules, suppression, and the repository settings to apply).
- New `.github/workflows/brain-gate-tests.yml` in this repository: it runs the fixture tests on changes to `gate/`.
- `brain-registry-sync`: Set up and Update gate write the caller and config through a batch PR. The Brains page shows each brain's gate version.
- `brain-log-capture`: renderer tests use `gate/tests` fixtures directly instead of copies.
- `sagehaven-docs` (other repository, follow-up ticket): replace `scripts/gate/` and the workflow with the caller and config. The checks' names stay the same, so `main`'s branch protection needs no change.
